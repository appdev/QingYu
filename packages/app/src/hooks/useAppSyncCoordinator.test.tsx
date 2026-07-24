import { StrictMode, type ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import type {
  SyncConfigDocument,
  SyncConfigLoadResult,
  SyncRunResult,
  SyncStatus
} from "../lib/sync-config";
import {
  emitSyncApplyRequested,
  emitSyncEditing,
  emitSyncStatusChanged
} from "../lib/sync-config-events";
import { dismissAppToast, showAppToast } from "../lib/app-toast";
import { appLogger } from "../lib/app-logger";
import { runApplicationSync } from "../lib/sync";
import { configureAppRuntime, createDefaultAppRuntime, resetAppRuntimeForTests } from "../runtime";
import { useAppSyncCoordinator } from "./useAppSyncCoordinator";

vi.mock("../lib/app-toast", () => ({ dismissAppToast: vi.fn(), showAppToast: vi.fn() }));
vi.mock("../lib/sync", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/sync")>(),
  runApplicationSync: vi.fn()
}));

const mockedRunApplicationSync = vi.mocked(runApplicationSync);
const mockedDismissAppToast = vi.mocked(dismissAppToast);
const mockedShowAppToast = vi.mocked(showAppToast);
let completeMockedApply: ((revision: string, token: string) => Promise<unknown>) | null = null;

function configDocument(revision = "rev-1", patch: Partial<SyncConfigDocument["config"]> = {}): SyncConfigDocument {
  return {
    config: {
      autoSyncOnSave: true,
      enabled: true,
      intervalMinutes: 0,
      provider: "webdav",
      remoteRoot: "qingyu",
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
      webdav: {
        password: "private",
        serverUrl: "https://dav.example.test",
        username: "writer"
      },
      ...patch
    },
    configured: true,
    issues: [],
    readiness: "ready",
    revision
  };
}

function syncResult(notesRoot: string, revision: string, trigger: SyncRunResult["trigger"]): SyncRunResult {
  return {
    notebookName: notesRoot.split(/[\\/]/).at(-1) ?? "",
    notesRoot,
    provider: "webdav",
    revision,
    summary: {
      bytesDownloaded: 1,
      bytesUploaded: 2,
      conflictFiles: 0,
      downloadedFiles: 1,
      scannedFiles: 3,
      skippedFiles: 0,
      uploadedFiles: 1
    },
    trigger
  };
}

function status(notesRoot: string, revision: string): {
  notebookName: string;
  revision: string;
  status: SyncStatus;
} {
  const notebookName = notesRoot.split(/[\\/]/).at(-1) ?? "";
  return {
    notebookName,
    revision,
    status: {
      completionState: "succeeded",
      error: null,
      lastAttemptAt: "2026-07-20T00:00:00Z",
      lastSuccessfulSyncAt: "2026-07-20T00:00:01Z",
      lastTrigger: "manual",
      notebookName,
      notesRoot,
      provider: "webdav",
      revision,
      summary: null,
      version: 1
    }
  };
}

function deferred<T>() {
  let resolve!: (value: T) => undefined;
  const promise = new Promise<T>((complete) => {
    resolve = (value) => {
      complete(value);
      return undefined;
    };
  });
  return { promise, resolve };
}

function installRuntime(membership = async (documentPath: string, rootPath: string) => (
  documentPath.startsWith(`${rootPath}/`)
)) {
  const runtime = createDefaultAppRuntime();
  const listeners = new Map<string, Set<(event: { payload: unknown }) => unknown>>();
  const cancelApply = vi.fn(runtime.syncConfig.cancelApply);
  const isDocumentInRoot = vi.fn(membership);
  configureAppRuntime({
    ...runtime,
    events: {
      emit: async (event, payload) => {
        for (const listener of listeners.get(event) ?? []) listener({ payload });
      },
      isAvailable: () => true,
      listen: async (event, listener) => {
        const registered = listeners.get(event) ?? new Set();
        registered.add(listener as (event: { payload: unknown }) => unknown);
        listeners.set(event, registered);
        return () => registered.delete(listener as (event: { payload: unknown }) => unknown);
      }
    },
    syncConfig: {
      ...runtime.syncConfig,
      cancelApply,
      loadStatus: async () => null
    },
    workspace: { ...runtime.workspace, isDocumentInRoot }
  });
  completeMockedApply = async (revision, token) => {
    const snapshot = await runtime.syncConfig.loadEditing();
    if (
      snapshot.pendingApply?.revision === revision &&
      snapshot.pendingApply.token === token &&
      snapshot.pendingApply.state !== "completed"
    ) {
      await runtime.syncConfig.cancelApply({
        revision,
        sessionId: snapshot.pendingApply.sessionId,
        token
      });
    }
  };
  return { cancelApply, isDocumentInRoot, syncConfig: runtime.syncConfig };
}

function renderCoordinator({
  document = configDocument(),
  onFilesChanged,
  primaryRoot = "/Notes",
  reload = vi.fn(async () => null)
}: {
  document?: SyncConfigDocument | null;
  onFilesChanged?: (root: string) => Promise<unknown> | unknown;
  primaryRoot?: string | null;
  reload?: () => Promise<SyncConfigLoadResult | null>;
} = {}) {
  return renderHook(
    ({ currentDocument, currentRoot }) => useAppSyncCoordinator({
      configDocument: currentDocument,
      onFilesChanged,
      primaryRoot: currentRoot,
      reloadConfig: reload,
      translate: (key) => key
    }),
    { initialProps: { currentDocument: document, currentRoot: primaryRoot } }
  );
}

describe("application sync coordinator", () => {
  beforeEach(() => {
    installRuntime();
    mockedRunApplicationSync.mockReset();
    mockedDismissAppToast.mockReset();
    mockedShowAppToast.mockReset();
    mockedRunApplicationSync.mockImplementation(async (input) => {
      if (!("notesRoot" in input)) throw new Error("application coordinator only issues normal sync requests");
      if ("applyToken" in input && input.applyToken) {
        await completeMockedApply?.(input.revision, input.applyToken);
      }
      return syncResult(input.notesRoot, input.revision, input.trigger);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    completeMockedApply = null;
    resetAppRuntimeForTests();
  });

  it("runs launch sync only for the active primary integration root", async () => {
    const { result, rerender } = renderCoordinator({ primaryRoot: null });
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();

    rerender({ currentDocument: configDocument(), currentRoot: "/Notes" });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "app-launch"
    }));
    await waitFor(() => expect(result.current.running).toBe(false));
  });

  it("cancels a queued old-root run and drains an already-started run before switching", async () => {
    const nativeRun = deferred<SyncRunResult>();
    mockedRunApplicationSync.mockImplementationOnce(() => nativeRun.promise);
    const { result, rerender } = renderCoordinator({ primaryRoot: "/Notes" });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "app-launch"
    }));

    rerender({ currentDocument: configDocument("rev-2"), currentRoot: "/Notes" });
    await Promise.resolve();
    let drained = false;
    const draining = result.current.beginNotebookSwitch().then(() => {
      drained = true;
    });
    await Promise.resolve();
    expect(drained).toBe(false);

    nativeRun.resolve(syncResult("/Notes", "rev-1", "app-launch"));
    await draining;
    result.current.finishNotebookSwitch();

    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);
  });

  it("does not carry an old running count into the replacement switch generation", async () => {
    const oldRun = deferred<SyncRunResult>();
    mockedRunApplicationSync.mockImplementationOnce(() => oldRun.promise);
    const { result } = renderCoordinator({ primaryRoot: "/Notes" });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "app-launch"
    }));
    await waitFor(() => expect(result.current.running).toBe(true));

    let draining!: Promise<void>;
    act(() => {
      draining = result.current.beginNotebookSwitch();
    });
    oldRun.resolve(syncResult("/Notes", "rev-1", "app-launch"));
    await act(async () => {
      await draining;
      result.current.finishNotebookSwitch();
    });

    const newRun = deferred<SyncRunResult>();
    mockedRunApplicationSync.mockImplementationOnce(() => newRun.promise);
    let nextRun!: Promise<SyncRunResult | null>;
    act(() => {
      nextRun = result.current.run("manual");
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenLastCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "manual"
    }));
    await waitFor(() => expect(result.current.running).toBe(true));

    newRun.resolve(syncResult("/Notes", "rev-1", "manual"));
    await act(async () => {
      await nextRun;
    });
    expect(result.current.running).toBe(false);
  });

  it("drains a started settings apply through its paired native publication", async () => {
    const reload = vi.fn(async () => ({ status: "loaded", ...configDocument("rev-2") } as SyncConfigLoadResult));
    const { result } = renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));
    mockedRunApplicationSync.mockClear();
    const nativePublication = deferred<SyncRunResult>();
    mockedRunApplicationSync.mockImplementationOnce(() => nativePublication.promise);

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));
    await act(() => emitSyncApplyRequested({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "s1",
      source: "settings-exit",
      token: "apply-drain"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-2", sessionId: "s1" }));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      applyToken: "apply-drain",
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-2",
      trigger: "settings-exit"
    }));

    let drained = false;
    const draining = result.current.beginNotebookSwitch().then(() => {
      drained = true;
    });
    await Promise.resolve();
    expect(drained).toBe(false);

    nativePublication.resolve(syncResult("/Notes", "rev-2", "settings-exit"));
    await draining;
    result.current.finishNotebookSwitch();
    expect(drained).toBe(true);
  });

  it("owns a settings apply from reload through publication across a failed same-root switch", async () => {
    const firstReload = deferred<SyncConfigLoadResult>();
    const reload = vi.fn()
      .mockImplementationOnce(() => firstReload.promise)
      .mockResolvedValue({ status: "loaded", ...configDocument("rev-3") } as SyncConfigLoadResult);
    const { result } = renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));
    mockedRunApplicationSync.mockClear();

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));
    await act(() => emitSyncApplyRequested({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "s1",
      source: "settings-exit",
      token: "apply-during-reload"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-2", sessionId: "s1" }));
    await waitFor(() => expect(reload).toHaveBeenCalledOnce());

    let drained = false;
    const draining = result.current.beginNotebookSwitch().then(() => {
      drained = true;
    });
    await Promise.resolve();
    const drainedBeforeReload = drained;
    firstReload.resolve({ status: "loaded", ...configDocument("rev-2") });
    await draining;
    result.current.finishNotebookSwitch();

    expect(drainedBeforeReload).toBe(false);
    expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      applyToken: "apply-during-reload",
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-2",
      trigger: "settings-exit"
    });

    mockedRunApplicationSync.mockClear();
    await act(() => emitSyncEditing({ active: true, revision: "rev-2", sessionId: "s2" }));
    await act(() => emitSyncApplyRequested({
      exitReason: "window-close",
      revision: "rev-3",
      sessionId: "s2",
      source: "settings-exit",
      token: "apply-after-failed-switch"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-3", sessionId: "s2" }));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      applyToken: "apply-after-failed-switch",
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-3",
      trigger: "settings-exit"
    }));
  });

  it("settles an apply requested while a failed same-root switch barrier is active", async () => {
    const { cancelApply, syncConfig } = installRuntime();
    const reload = vi.fn(async () => ({ status: "loaded", ...configDocument("rev-2") } as SyncConfigLoadResult));
    const { result } = renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));
    mockedRunApplicationSync.mockClear();

    const editing = await syncConfig.setEditing({
      active: true,
      revision: "rev-1",
      sessionId: "barrier-session"
    });
    await act(() => emitSyncEditing(editing.event));
    await act(async () => {
      await result.current.beginNotebookSwitch();
    });
    const apply = await syncConfig.requestApply({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "barrier-session",
      source: "settings-exit",
      token: "apply-inside-barrier"
    });
    await act(() => emitSyncApplyRequested(apply.event));
    const finishedEditing = await syncConfig.setEditing({
      active: false,
      revision: "rev-2",
      sessionId: "barrier-session"
    });
    await act(() => emitSyncEditing(finishedEditing.event));
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.finishNotebookSwitch();
    });

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      applyToken: "apply-inside-barrier",
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-2",
      trigger: "settings-exit"
    }));
    expect(cancelApply).not.toHaveBeenCalled();
  });

  it("does not publish an old-root barrier apply after the notebook root changes", async () => {
    const { cancelApply, syncConfig } = installRuntime();
    const reload = vi.fn(async () => ({ status: "loaded", ...configDocument("rev-2") } as SyncConfigLoadResult));
    const { result, rerender } = renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));
    mockedRunApplicationSync.mockClear();

    const editing = await syncConfig.setEditing({
      active: true,
      revision: "rev-1",
      sessionId: "old-session"
    });
    await act(() => emitSyncEditing(editing.event));
    await act(async () => {
      await result.current.beginNotebookSwitch();
    });
    const oldApply = await syncConfig.requestApply({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "old-session",
      source: "settings-exit",
      token: "old-root-apply"
    });
    await act(() => emitSyncApplyRequested(oldApply.event));
    const finishedEditing = await syncConfig.setEditing({
      active: false,
      revision: "rev-2",
      sessionId: "old-session"
    });
    await act(() => emitSyncEditing(finishedEditing.event));

    rerender({ currentDocument: configDocument("rev-2"), currentRoot: "/Other" });
    await act(async () => {
      await result.current.finishNotebookSwitch();
    });
    expect(cancelApply).toHaveBeenCalledOnce();
    expect(cancelApply).toHaveBeenCalledWith({
      revision: "rev-2",
      sessionId: "old-session",
      token: "old-root-apply"
    });
    expect((await syncConfig.loadEditing()).pendingApply).toEqual(expect.objectContaining({
      state: "completed",
      token: "old-root-apply"
    }));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      notesRoot: "/Other",
      trigger: "app-launch"
    })));

    expect(mockedRunApplicationSync).not.toHaveBeenCalledWith(expect.objectContaining({
      applyToken: "old-root-apply",
      trigger: "settings-exit"
    }));

    const nextEditing = await syncConfig.setEditing({
      active: true,
      revision: "rev-3",
      sessionId: "new-session"
    });
    await act(() => emitSyncEditing(nextEditing.event));
    const nextApply = await syncConfig.requestApply({
      exitReason: "window-close",
      revision: "rev-3",
      sessionId: "new-session",
      source: "settings-exit",
      token: "new-root-apply"
    });
    await act(() => emitSyncApplyRequested(nextApply.event));
    await act(() => emitSyncApplyRequested(nextApply.event));
    const nextFinished = await syncConfig.setEditing({
      active: false,
      revision: "rev-3",
      sessionId: "new-session"
    });
    await act(() => emitSyncEditing(nextFinished.event));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      applyToken: "new-root-apply",
      notesRoot: "/Other",
      revision: "rev-3",
      trigger: "settings-exit"
    })));
    expect(mockedRunApplicationSync.mock.calls.filter(([request]) => (
      "applyToken" in request && request.applyToken === "new-root-apply"
    ))).toHaveLength(1);

    await waitFor(() => expect(result.current.running).toBe(false));
    mockedRunApplicationSync.mockClear();
    await act(async () => {
      await result.current.beginNotebookSwitch();
      await result.current.finishNotebookSwitch();
    });
    expect(mockedRunApplicationSync).not.toHaveBeenCalledWith(expect.objectContaining({
      applyToken: "old-root-apply",
      trigger: "settings-exit"
    }));
  });

  it("reconciles an eventless native apply created during the root listener handoff", async () => {
    const { cancelApply, syncConfig } = installRuntime();
    const cancellation = deferred<undefined>();
    cancelApply.mockImplementation(async (input) => {
      await cancellation.promise;
      return syncConfig.cancelApply(input);
    });
    const reload = vi.fn(async () => ({ status: "loaded", ...configDocument("rev-3") } as SyncConfigLoadResult));
    const { result, rerender } = renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));
    mockedRunApplicationSync.mockClear();

    await syncConfig.setEditing({ active: true, revision: "rev-1", sessionId: "handoff-session" });
    await act(async () => {
      await result.current.beginNotebookSwitch();
    });
    rerender({ currentDocument: configDocument("rev-2"), currentRoot: "/Other" });
    await syncConfig.requestApply({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "handoff-session",
      source: "settings-exit",
      token: "eventless-old-apply"
    });

    let finishing!: Promise<unknown>;
    act(() => {
      finishing = Promise.resolve(result.current.finishNotebookSwitch());
    });
    await waitFor(() => expect(cancelApply).toHaveBeenCalledWith({
      revision: "rev-2",
      sessionId: "handoff-session",
      token: "eventless-old-apply"
    }));
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();

    cancellation.resolve(undefined);
    await act(async () => finishing);
    expect((await syncConfig.loadEditing()).pendingApply).toEqual(expect.objectContaining({
      state: "completed",
      token: "eventless-old-apply"
    }));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      notesRoot: "/Other",
      trigger: "app-launch"
    })));

    await act(() => emitSyncEditing({ active: true, revision: "rev-2", sessionId: "new-session" }));
    const next = await syncConfig.requestApply({
      exitReason: "window-close",
      revision: "rev-3",
      sessionId: "new-session",
      source: "settings-exit",
      token: "new-root-apply-after-handoff"
    });
    await act(() => emitSyncApplyRequested(next.event));
    await act(() => emitSyncApplyRequested(next.event));
    await act(() => emitSyncEditing({ active: false, revision: "rev-3", sessionId: "new-session" }));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      applyToken: "new-root-apply-after-handoff",
      notesRoot: "/Other",
      revision: "rev-3",
      trigger: "settings-exit"
    })));
    expect(mockedRunApplicationSync.mock.calls.filter(([request]) => (
      "applyToken" in request && request.applyToken === "new-root-apply-after-handoff"
    ))).toHaveLength(1);
  });

  it("does not clear a newer native token when exact handoff cancellation races", async () => {
    const { cancelApply, syncConfig } = installRuntime();
    cancelApply.mockImplementationOnce(async (input) => {
      await syncConfig.cancelApply(input);
      await syncConfig.setEditing({ active: true, revision: "rev-3", sessionId: "race-new-session" });
      await syncConfig.requestApply({
        exitReason: "window-close",
        revision: "rev-3",
        sessionId: "race-new-session",
        source: "settings-exit",
        token: "race-new-token"
      });
      return syncConfig.cancelApply(input);
    });
    const { result, rerender } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));

    await syncConfig.setEditing({ active: true, revision: "rev-1", sessionId: "race-old-session" });
    await act(async () => {
      await result.current.beginNotebookSwitch();
    });
    rerender({ currentDocument: configDocument("rev-2"), currentRoot: "/Other" });
    await syncConfig.requestApply({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "race-old-session",
      source: "settings-exit",
      token: "race-old-token"
    });

    await act(async () => {
      await result.current.finishNotebookSwitch();
    });

    expect(cancelApply).toHaveBeenCalledWith({
      revision: "rev-2",
      sessionId: "race-old-session",
      token: "race-old-token"
    });
    expect((await syncConfig.loadEditing()).pendingApply).toEqual(expect.objectContaining({
      revision: "rev-3",
      sessionId: "race-new-session",
      state: "pending",
      token: "race-new-token"
    }));
  });

  it("publishes only the newest valid barrier apply when events repeat", async () => {
    const reload = vi.fn(async () => ({ status: "loaded", ...configDocument("rev-3") } as SyncConfigLoadResult));
    const { result } = renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));
    mockedRunApplicationSync.mockClear();

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "repeat-session" }));
    await act(async () => {
      await result.current.beginNotebookSwitch();
    });
    await act(() => emitSyncApplyRequested({
      exitReason: "category-leave",
      revision: "rev-3",
      sessionId: "repeat-session",
      source: "settings-exit",
      token: "newest-apply"
    }));
    for (const _repeat of [1, 2]) {
      await act(() => emitSyncApplyRequested({
        exitReason: "category-leave",
        revision: "rev-3",
        sessionId: "repeat-session",
        source: "settings-exit",
        token: "newest-apply"
      }));
    }
    await act(() => emitSyncEditing({ active: false, revision: "rev-3", sessionId: "repeat-session" }));

    await act(async () => {
      await result.current.finishNotebookSwitch();
    });

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      applyToken: "newest-apply",
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-3",
      trigger: "settings-exit"
    }));
    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);
  });

  it("rejects a native result whose immutable notebook name differs from its request", async () => {
    const changed = vi.fn();
    const { result } = renderCoordinator({ onFilesChanged: changed });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await waitFor(() => expect(result.current.running).toBe(false));
    changed.mockClear();
    mockedRunApplicationSync.mockResolvedValueOnce({
      ...syncResult("/Notes", "rev-1", "manual"),
      notebookName: "Other"
    });

    let returned: SyncRunResult | null = syncResult("/placeholder", "rev", "manual");
    await act(async () => {
      returned = await result.current.run("manual");
    });

    expect(returned).toBeNull();
    expect(changed).not.toHaveBeenCalled();
  });

  it("checks native membership after a successful save and ignores external files", async () => {
    const { isDocumentInRoot } = installRuntime(async (documentPath, rootPath) => (
      documentPath === `${rootPath}/inside.md`
    ));
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();

    await act(() => result.current.notifyDocumentSaved("/External/file.md"));
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    await act(() => result.current.notifyDocumentSaved("/Notes/inside.md"));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "save"
    }));
    expect(isDocumentInRoot).toHaveBeenCalledWith("/Notes/inside.md", "/Notes");
  });

  it("pauses automatic triggers until the edited settings session exits", async () => {
    const reload = vi.fn(async () => ({ status: "loaded", ...configDocument("rev-2") } as SyncConfigLoadResult));
    const { result } = renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    await act(() => emitSyncApplyRequested({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "s1",
      source: "settings-exit",
      token: "apply-1"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-2", sessionId: "s1" }));

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      applyToken: "apply-1",
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-2",
      trigger: "settings-exit"
    }));
  });

  it("settles a settings apply through native sync even when the frontend reload fails", async () => {
    const reload = vi.fn(async () => null);
    renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));
    await act(() => emitSyncApplyRequested({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "s1",
      source: "settings-exit",
      token: "apply-reload-failed"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-2", sessionId: "s1" }));

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      applyToken: "apply-reload-failed",
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-2",
      trigger: "settings-exit"
    }));
  });

  it("cancels queued old-root work and prevents an in-flight old result from updating the new root", async () => {
    const runA = deferred<SyncRunResult>();
    const changed = vi.fn();
    mockedRunApplicationSync.mockImplementationOnce(() => runA.promise);
    const { result, rerender } = renderCoordinator({ onFilesChanged: changed, primaryRoot: "/A" });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({ notesRoot: "/A" })));

    rerender({ currentDocument: configDocument("rev-b"), currentRoot: "/B" });
    expect(result.current.status).toBeNull();
    await act(async () => {
      runA.resolve(syncResult("/A", "rev-1", "app-launch"));
      await runA.promise;
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      notesRoot: "/B",
      revision: "rev-b"
    })));
    expect(changed).not.toHaveBeenCalledWith("/A");
  });

  it("ignores a stale status event from a previous root or revision", async () => {
    const { result } = renderCoordinator({ primaryRoot: "/B", document: configDocument("rev-b") });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    await act(() => emitSyncStatusChanged({ notesRoot: "/A", ...status("/A", "rev-b") }));
    await act(() => emitSyncStatusChanged({ notesRoot: "/B", ...status("/B", "rev-old") }));
    expect(result.current.status).toBeNull();

    await act(() => emitSyncStatusChanged({ notesRoot: "/B", ...status("/B", "rev-b") }));
    expect(result.current.status?.notesRoot).toBe("/B");
  });

  it("fails closed immediately when a ready configuration becomes unavailable", async () => {
    const { result, rerender } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();

    rerender({ currentDocument: null, currentRoot: "/Notes" });
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await act(() => result.current.run("manual"));

    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
  });

  it("runs the configured interval and invalidates the old timer after a root switch", async () => {
    const setIntervalSpy = vi.spyOn(window, "setInterval");
    const { rerender } = renderCoordinator({
      document: configDocument("rev-a", { intervalMinutes: 5 }),
      primaryRoot: "/A"
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      notesRoot: "/A",
      trigger: "app-launch"
    })));
    const oldTimer = setIntervalSpy.mock.calls.find(([, delay]) => delay === 5 * 60 * 1000)?.[0];
    expect(oldTimer).toBeTypeOf("function");
    mockedRunApplicationSync.mockClear();

    await act(async () => {
      if (typeof oldTimer === "function") await oldTimer();
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      notebookName: "A",
      notesRoot: "/A",
      revision: "rev-a",
      trigger: "interval"
    }));

    rerender({
      currentDocument: configDocument("rev-b", { intervalMinutes: 5 }),
      currentRoot: "/B"
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      notesRoot: "/B",
      trigger: "app-launch"
    })));
    mockedRunApplicationSync.mockClear();
    await act(async () => {
      if (typeof oldTimer === "function") await oldTimer();
    });
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    setIntervalSpy.mockRestore();
  });

  it("coalesces save and manual callers by root and revision while preserving the manual result", async () => {
    const pending = deferred<SyncRunResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync.mockImplementationOnce(() => pending.promise);
    let manualRun: Promise<SyncRunResult | null> | null = null;

    await act(async () => {
      await result.current.notifyDocumentSaved("/Notes/file.md");
      manualRun = result.current.run("manual");
    });
    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);
    expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "save"
    });

    let manualResult: SyncRunResult | null = null;
    await act(async () => {
      pending.resolve(syncResult("/Notes", "rev-1", "save"));
      manualResult = await manualRun;
    });
    expect(manualResult).toEqual(expect.objectContaining({
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "manual"
    }));
  });

  it("queues one fresh save pass when a document is saved during an active sync", async () => {
    const firstSave = deferred<SyncRunResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync
      .mockImplementationOnce(() => firstSave.promise)
      .mockResolvedValueOnce(syncResult("/Notes", "rev-1", "save"));

    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);

    await act(async () => {
      firstSave.resolve(syncResult("/Notes", "rev-1", "save"));
      await firstSave.promise;
    });

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2));
    expect(mockedRunApplicationSync).toHaveBeenLastCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "save"
    });
  });

  it("queues later saves behind the bounded trailing save pass", async () => {
    const firstSave = deferred<SyncRunResult>();
    const trailingSave = deferred<SyncRunResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementationOnce(() => trailingSave.promise)
      .mockResolvedValueOnce(syncResult("/Notes", "rev-1", "save"));

    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await act(async () => {
      firstSave.resolve(syncResult("/Notes", "rev-1", "save"));
      await firstSave.promise;
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2));

    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2);
    await act(async () => {
      trailingSave.resolve(syncResult("/Notes", "rev-1", "save"));
      await trailingSave.promise;
    });

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(3));
  });

  it("uses save eligibility when a save joins an active manual run", async () => {
    const manual = deferred<SyncRunResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync.mockImplementationOnce(() => manual.promise);

    let manualRun!: Promise<SyncRunResult | null>;
    await act(() => {
      manualRun = result.current.run("manual");
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));

    await act(async () => {
      manual.resolve(syncResult("/Notes", "rev-1", "manual"));
      await manualRun;
    });

    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);
  });

  it("does not turn a failed primary run into an automatic save retry", async () => {
    let rejectPrimary!: (error: Error) => undefined;
    const primary = new Promise<SyncRunResult>((_, reject) => {
      rejectPrimary = (error) => {
        reject(error);
        return undefined;
      };
    });
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync.mockImplementationOnce(() => primary);

    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));

    await act(async () => {
      rejectPrimary(new Error("s3-upload-request-failed: exhausted retry budget"));
      await primary.catch(() => undefined);
    });

    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      message: "settings.sync.toastIncomplete",
      status: "error"
    })));
    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);
  });

  it("shows one safe failure notification for a visible application sync", async () => {
    mockedRunApplicationSync.mockRejectedValueOnce(
      new Error("remote-http-error: synthetic safe failure")
    );

    renderCoordinator();

    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      action: expect.objectContaining({ label: "settings.sync.toastRetry" }),
      id: "app-sync",
      message: "settings.sync.toastIncomplete",
      presentation: "sync-error",
      status: "error"
    })));
    expect(mockedShowAppToast.mock.calls[0]?.[0]).not.toHaveProperty("description");
    expect(mockedShowAppToast).toHaveBeenCalledTimes(1);
    expect(mockedShowAppToast.mock.calls.filter(([toast]) => toast.status === "error")).toHaveLength(1);
  });

  it("shows structured S3 diagnostics recovered from the persisted failed status", async () => {
    const logSpy = vi.spyOn(appLogger, "error");
    const runtime = createDefaultAppRuntime();
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        loadStatus: async () => ({
          completionState: "failed",
          error: {
            category: "http",
            code: "s3-upload-http-failed",
            httpStatus: 403,
            method: "PUT",
            objectId: "object-a1",
            operation: "upload",
            provider: "s3",
            providerErrorCode: "AccessDenied",
            relativePath: null,
            requestId: "request-403",
            runId: "run-1"
          },
          lastAttemptAt: "2026-07-23T01:53:04Z",
          lastSuccessfulSyncAt: null,
          lastTrigger: "app-launch",
          notebookName: "Notes",
          notesRoot: "/Notes",
          provider: "s3",
          revision: "rev-1",
          summary: null,
          version: 1
        })
      },
      workspace: { ...runtime.workspace, isDocumentInRoot: async () => true }
    });
    mockedRunApplicationSync.mockRejectedValueOnce(
      new Error("s3-upload-http-failed: Application synchronization did not complete.")
    );

    renderCoordinator({ document: configDocument("rev-1", { provider: "s3" }) });

    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      action: expect.objectContaining({ label: "settings.sync.toastRetry" }),
      id: "app-sync",
      message: "settings.sync.toastIncomplete",
      presentation: "sync-error",
      status: "error"
    })));
    expect(mockedShowAppToast.mock.calls[0]?.[0]).not.toHaveProperty("description");
    expect(logSpy).toHaveBeenCalledWith("sync", "Application synchronization failed", {
      category: "http",
      code: "s3-upload-http-failed",
      httpStatus: 403,
      method: "PUT",
      objectId: "object-a1",
      operation: "upload",
      provider: "s3",
      providerErrorCode: "AccessDenied",
      requestId: "request-403",
      runId: "run-1"
    });
    logSpy.mockRestore();
  });

  it("retries the current sync in place and dismisses the toast after success", async () => {
    mockedRunApplicationSync
      .mockRejectedValueOnce(new Error("remote-http-error: initial failure"))
      .mockResolvedValueOnce(syncResult("/Notes", "rev-1", "manual"));

    renderCoordinator();

    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      action: expect.any(Object),
      message: "settings.sync.toastIncomplete",
      status: "error"
    })));
    const action = mockedShowAppToast.mock.calls.at(-1)?.[0].action;
    if (!action || typeof action !== "object" || !("onClick" in action)) {
      throw new Error("Expected the sync failure toast to expose a retry action.");
    }
    const preventDefault = vi.fn();

    act(() => {
      action.onClick({ preventDefault } as never);
    });

    expect(preventDefault).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      id: "app-sync",
      message: "settings.sync.toastRetrying",
      presentation: "sync-error",
      status: "loading"
    })));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2));
    expect(mockedRunApplicationSync).toHaveBeenLastCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "manual"
    });
    await waitFor(() => expect(mockedDismissAppToast).toHaveBeenCalledWith("app-sync"));
  });

  it("restores one transient error toast when retrying fails", async () => {
    mockedRunApplicationSync
      .mockRejectedValueOnce(new Error("remote-http-error: initial failure"))
      .mockRejectedValueOnce(new Error("remote-http-error: retry failure"));

    renderCoordinator();

    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      action: expect.any(Object),
      status: "error"
    })));
    const action = mockedShowAppToast.mock.calls.at(-1)?.[0].action;
    if (!action || typeof action !== "object" || !("onClick" in action)) {
      throw new Error("Expected the sync failure toast to expose a retry action.");
    }

    act(() => {
      action.onClick({ preventDefault: vi.fn() } as never);
    });

    await waitFor(() => expect(
      mockedShowAppToast.mock.calls.filter(([toast]) => toast.status === "error")
    ).toHaveLength(2));
    expect(mockedShowAppToast.mock.calls.filter(([toast]) => toast.status === "error"))
      .toEqual(expect.arrayContaining([
        [expect.objectContaining({ id: "app-sync", presentation: "sync-error" })],
        [expect.objectContaining({ id: "app-sync", presentation: "sync-error" })]
      ]));
    expect(mockedDismissAppToast).not.toHaveBeenCalled();
  });

  it("does not let a stale retry success dismiss a newer workspace failure", async () => {
    const retry = deferred<SyncRunResult>();
    installRuntime(async () => {
      throw new Error("workspace-document-membership-unavailable");
    });
    mockedRunApplicationSync
      .mockRejectedValueOnce(new Error("remote-http-error: initial failure"))
      .mockImplementationOnce(() => retry.promise)
      .mockResolvedValueOnce(syncResult("/B", "rev-b", "app-launch"));

    const { result, rerender } = renderCoordinator();
    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      action: expect.any(Object),
      status: "error"
    })));
    const action = mockedShowAppToast.mock.calls.at(-1)?.[0].action;
    if (!action || typeof action !== "object" || !("onClick" in action)) {
      throw new Error("Expected the sync failure toast to expose a retry action.");
    }
    act(() => action.onClick({ preventDefault: vi.fn() } as never));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2));

    rerender({ currentDocument: configDocument("rev-b"), currentRoot: "/B" });
    await act(async () => {
      await result.current.notifyDocumentSaved("/B/draft.md");
    });
    await waitFor(() => expect(
      mockedShowAppToast.mock.calls.filter(([toast]) => toast.status === "error")
    ).toHaveLength(2));
    const dismissCountBeforeRetrySettles = mockedDismissAppToast.mock.calls.length;

    await act(async () => {
      retry.resolve(syncResult("/Notes", "rev-1", "manual"));
      await retry.promise;
    });

    expect(mockedDismissAppToast).toHaveBeenCalledTimes(dismissCountBeforeRetrySettles);
  });

  it("clears an owned loading toast on workspace change without a second stale dismissal", async () => {
    const retry = deferred<SyncRunResult>();
    mockedRunApplicationSync
      .mockRejectedValueOnce(new Error("remote-http-error: initial failure"))
      .mockImplementationOnce(() => retry.promise)
      .mockResolvedValueOnce(syncResult("/B", "rev-b", "app-launch"));

    const { rerender } = renderCoordinator();
    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      action: expect.any(Object),
      status: "error"
    })));
    const action = mockedShowAppToast.mock.calls.at(-1)?.[0].action;
    if (!action || typeof action !== "object" || !("onClick" in action)) {
      throw new Error("Expected the sync failure toast to expose a retry action.");
    }
    act(() => action.onClick({ preventDefault: vi.fn() } as never));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2));

    rerender({ currentDocument: configDocument("rev-b"), currentRoot: "/B" });
    await waitFor(() => expect(mockedDismissAppToast).toHaveBeenCalledTimes(1));

    await act(async () => {
      retry.resolve(syncResult("/Notes", "rev-1", "manual"));
      await retry.promise;
    });
    expect(mockedDismissAppToast).toHaveBeenCalledTimes(1);
  });

  it("restores the failure toast when a retry is cancelled before it starts", async () => {
    mockedRunApplicationSync.mockRejectedValueOnce(
      new Error("remote-http-error: initial failure")
    );
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      action: expect.any(Object),
      status: "error"
    })));
    const action = mockedShowAppToast.mock.calls.at(-1)?.[0].action;
    if (!action || typeof action !== "object" || !("onClick" in action)) {
      throw new Error("Expected the sync failure toast to expose a retry action.");
    }
    await act(async () => {
      await result.current.beginNotebookSwitch();
    });

    act(() => action.onClick({ preventDefault: vi.fn() } as never));

    try {
      await waitFor(() => expect(
        mockedShowAppToast.mock.calls.filter(([toast]) => toast.status === "error")
      ).toHaveLength(2));
    } finally {
      await act(async () => {
        await result.current.finishNotebookSwitch();
      });
    }
    expect(mockedDismissAppToast).not.toHaveBeenCalled();
  });

  it("does not show a notification for a successful application sync", async () => {
    const { result } = renderCoordinator();

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(result.current.running).toBe(false));
    expect(mockedShowAppToast).not.toHaveBeenCalled();
  });

  it("fails automatic sync closed when editing state is unavailable but still allows manual sync", async () => {
    const runtime = createDefaultAppRuntime();
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async () => undefined,
        isAvailable: () => true,
        listen: async () => () => undefined
      },
      syncConfig: {
        ...runtime.syncConfig,
        loadEditing: async () => {
          throw new Error("synthetic editing registry failure");
        },
        loadStatus: async () => null
      },
      workspace: { ...runtime.workspace, isDocumentInRoot: async () => true }
    });
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      message: "settings.sync.toastIncomplete",
      presentation: "sync-error",
      status: "error"
    })));
    expect(mockedShowAppToast.mock.calls[0]?.[0]).not.toHaveProperty("description");
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();

    await act(() => result.current.run("manual"));

    expect(mockedRunApplicationSync).toHaveBeenCalledWith({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "manual"
    });
  });

  it("survives StrictMode remount probing and cleans every registration exactly once", async () => {
    const runtime = createDefaultAppRuntime();
    const cleanups: Array<ReturnType<typeof vi.fn>> = [];
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async () => undefined,
        isAvailable: () => true,
        listen: async () => {
          const cleanup = vi.fn();
          cleanups.push(cleanup);
          return cleanup;
        }
      },
      syncConfig: {
        ...runtime.syncConfig,
        loadEditing: async () => ({ counter: 0, pendingApply: null, state: null }),
        loadStatus: async () => null
      },
      workspace: { ...runtime.workspace, isDocumentInRoot: async () => true }
    });
    const wrapper = ({ children }: { children: ReactNode }) => <StrictMode>{children}</StrictMode>;
    const { unmount } = renderHook(() => useAppSyncCoordinator({
      configDocument: configDocument(),
      primaryRoot: "/Notes",
      reloadConfig: async () => null,
      translate: (key) => key
    }), { wrapper });

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    unmount();
    await waitFor(() => expect(cleanups.length).toBeGreaterThanOrEqual(4));
    expect(cleanups.every((cleanup) => cleanup.mock.calls.length === 1)).toBe(true);
  });

  it("cleans delayed listener registrations once when unmounted before registration finishes", async () => {
    const runtime = createDefaultAppRuntime();
    const registration = deferred<undefined>();
    const cleanups: Array<ReturnType<typeof vi.fn>> = [];
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async () => undefined,
        isAvailable: () => true,
        listen: async () => {
          await registration.promise;
          const cleanup = vi.fn();
          cleanups.push(cleanup);
          return cleanup;
        }
      },
      syncConfig: {
        ...runtime.syncConfig,
        loadEditing: async () => ({ counter: 0, pendingApply: null, state: null }),
        loadStatus: async () => null
      }
    });
    const { unmount } = renderCoordinator();
    unmount();
    await act(async () => {
      registration.resolve(undefined);
      await registration.promise;
    });
    await waitFor(() => expect(cleanups).toHaveLength(4));
    expect(cleanups.every((cleanup) => cleanup.mock.calls.length === 1)).toBe(true);
  });

  it("cleans completed listener registrations without waiting for the slowest registration", async () => {
    const runtime = createDefaultAppRuntime();
    const delayedRegistration = deferred<undefined>();
    const cleanups: Array<ReturnType<typeof vi.fn>> = [];
    let registrations = 0;
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async () => undefined,
        isAvailable: () => true,
        listen: async () => {
          registrations += 1;
          if (registrations === 1) await delayedRegistration.promise;
          const cleanup = vi.fn();
          cleanups.push(cleanup);
          return cleanup;
        }
      },
      syncConfig: {
        ...runtime.syncConfig,
        loadEditing: async () => ({ counter: 0, pendingApply: null, state: null }),
        loadStatus: async () => null
      }
    });
    const { unmount } = renderCoordinator();
    await waitFor(() => expect(cleanups).toHaveLength(3));

    unmount();
    expect(cleanups.every((cleanup) => cleanup.mock.calls.length === 1)).toBe(true);

    await act(async () => {
      delayedRegistration.resolve(undefined);
      await delayedRegistration.promise;
    });
    await waitFor(() => expect(cleanups).toHaveLength(4));
    expect(cleanups.every((cleanup) => cleanup.mock.calls.length === 1)).toBe(true);
  });
});
