import { StrictMode, type ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import type {
  DejavuRepositoryStatus,
  SyncConfigDocument,
  SyncConfigLoadResult,
  SyncDispatchResult,
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
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  getAppRuntime,
  resetAppRuntimeForTests
} from "../runtime";
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
      enabled: true,
      intervalSeconds: 30,
      generateConflictDocument: false,
      mode: "automatic",
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
      version: 3,
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

function legacySyncResult(notesRoot: string, revision: string, trigger: SyncRunResult["trigger"]): SyncRunResult {
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

function completedDispatch(
  notesRoot: string,
  revision: string,
  trigger: SyncRunResult["trigger"]
): SyncDispatchResult {
  return { result: legacySyncResult(notesRoot, revision, trigger), status: "completed" };
}

function acceptedDispatch(
  notesRoot: string,
  identity: Partial<Extract<SyncDispatchResult, { status: "accepted" }>["job"]> = {}
): SyncDispatchResult {
  return {
    job: {
      jobId: "00000000-0000-4000-8000-000000000401",
      notesRoot,
      repositoryId: "00000000-0000-4000-8000-000000000402",
      ...identity
    },
    status: "accepted"
  };
}

function dejavuStatus(
  phase: DejavuRepositoryStatus["phase"],
  error: DejavuRepositoryStatus["error"] = null,
  identity: Partial<Pick<DejavuRepositoryStatus, "jobId" | "repositoryId">> = {}
): DejavuRepositoryStatus {
  return {
    attempt: 1,
    automaticFailureCount: 0,
    conflicts: [],
    error,
    jobId: identity.jobId ?? "00000000-0000-4000-8000-000000000401",
    lastAttemptAt: "2026-07-28T00:00:00Z",
    lastDnsRetryAt: null,
    lastSuccessfulSyncAt: phase === "succeeded" ? "2026-07-28T00:00:01Z" : null,
    maintenance: {
      lastLocalPurgeAt: null,
      nextLocalPurgeAt: null
    },
    nextScheduledAt: null,
    phase,
    repositoryId: identity.repositoryId ?? "00000000-0000-4000-8000-000000000402",
    sameCount: 0,
    transfer: {
      downloadBytes: 0,
      downloadChunks: 0,
      downloadFiles: 0,
      uploadBytes: 0,
      uploadChunks: 0,
      uploadFiles: 0
    },
    trigger: "manual",
    version: 1
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
  dejavuSyncAvailable = true,
  document = configDocument(),
  onFilesChanged,
  primaryRoot = "/Notes",
  reload = vi.fn(async () => null)
}: {
  dejavuSyncAvailable?: boolean;
  document?: SyncConfigDocument | null;
  onFilesChanged?: (root: string) => Promise<unknown> | unknown;
  primaryRoot?: string | null;
  reload?: () => Promise<SyncConfigLoadResult | null>;
} = {}) {
  return renderHook(
    ({ currentDocument, currentRoot }) => useAppSyncCoordinator({
      configDocument: currentDocument,
      dejavuSyncAvailable,
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
      return completedDispatch(input.notesRoot, input.revision, input.trigger);
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

  it("keeps an accepted S3 job nonterminal until its matching Dejavu success event", async () => {
    const onFilesChanged = vi.fn();
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });
    let returned: SyncDispatchResult | null = null;

    await act(async () => {
      returned = await result.current.run("manual");
    });

    expect(returned).toEqual(acceptedDispatch("/Notes"));
    expect(onFilesChanged).not.toHaveBeenCalled();
    expect(result.current.running).toBe(true);

    await act(() => getAppRuntime().events.emit(
      "qingyu://dejavu-sync-status-changed",
      dejavuStatus("succeeded")
    ));

    await waitFor(() => expect(result.current.running).toBe(false));
    expect(onFilesChanged).toHaveBeenCalledOnce();
    expect(onFilesChanged).toHaveBeenCalledWith("/Notes");
  });

  it("returns an accepted basic S3 run without invoking Dejavu status recovery when unavailable", async () => {
    const runtime = getAppRuntime();
    const loadRepositoryStatus = vi.fn(runtime.syncConfig.loadRepositoryStatus);
    runtime.syncConfig.loadRepositoryStatus = loadRepositoryStatus;
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result } = renderCoordinator({
      dejavuSyncAvailable: false,
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      })
    });

    await act(async () => {
      await expect(result.current.run("manual")).resolves.toEqual(acceptedDispatch("/Notes"));
    });

    expect(result.current.running).toBe(false);
    expect(loadRepositoryStatus).not.toHaveBeenCalled();
  });

  it("reports a terminal Dejavu failure after S3 dispatch acceptance", async () => {
    const onFilesChanged = vi.fn();
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });

    await act(() => result.current.run("manual"));
    expect(mockedShowAppToast).not.toHaveBeenCalledWith(
      expect.objectContaining({ status: "error" })
    );

    await act(() => getAppRuntime().events.emit(
      "qingyu://dejavu-sync-status-changed",
      dejavuStatus("failed", {
        code: "dejavu-cloud-unavailable",
        operation: "repository-sync"
      })
    ));

    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "app-sync",
        message: "settings.sync.toastIncomplete",
        status: "error"
      })
    ));
    expect(result.current.running).toBe(false);
    expect(onFilesChanged).not.toHaveBeenCalled();
  });

  it("ignores a terminal Dejavu event for another jobId", async () => {
    const onFilesChanged = vi.fn();
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });

    await act(() => result.current.run("manual"));
    await act(() => getAppRuntime().events.emit(
      "qingyu://dejavu-sync-status-changed",
      {
        ...dejavuStatus("succeeded"),
        jobId: "00000000-0000-4000-8000-000000000499"
      }
    ));

    expect(result.current.running).toBe(true);
    expect(onFilesChanged).not.toHaveBeenCalled();

    await act(() => getAppRuntime().events.emit(
      "qingyu://dejavu-sync-status-changed",
      dejavuStatus("succeeded")
    ));

    await waitFor(() => expect(result.current.running).toBe(false));
    expect(onFilesChanged).toHaveBeenCalledOnce();
  });

  it("settles from a terminal Dejavu event that races ahead of acceptance", async () => {
    const onFilesChanged = vi.fn();
    const submission = deferred<SyncDispatchResult>();
    mockedRunApplicationSync.mockImplementationOnce(() => submission.promise);
    const { result } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });
    await act(async () => Promise.resolve());

    let run!: Promise<SyncDispatchResult | null>;
    act(() => {
      run = result.current.run("manual");
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledOnce());

    await act(() => getAppRuntime().events.emit(
      "qingyu://dejavu-sync-status-changed",
      dejavuStatus("succeeded")
    ));
    expect(onFilesChanged).not.toHaveBeenCalled();

    await act(async () => {
      submission.resolve(acceptedDispatch("/Notes"));
      await run;
    });

    await waitFor(() => expect(result.current.running).toBe(false));
    expect(onFilesChanged).toHaveBeenCalledOnce();
    expect(onFilesChanged).toHaveBeenCalledWith("/Notes");
  });

  it("recovers a terminal Dejavu status missed before listener registration", async () => {
    const runtime = getAppRuntime();
    const registration = deferred<undefined>();
    const loadRepositoryStatus = vi.fn(async () => dejavuStatus("succeeded"));
    configureAppRuntime({
      ...runtime,
      events: {
        ...runtime.events,
        listen: async (event, listener) => {
          await registration.promise;
          return runtime.events.listen(event, listener);
        }
      },
      syncConfig: {
        ...runtime.syncConfig,
        loadRepositoryStatus
      }
    });
    const onFilesChanged = vi.fn();
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });

    await act(() => result.current.run("manual"));
    await act(async () => {
      registration.resolve(undefined);
      await registration.promise;
    });

    await waitFor(() => expect(result.current.running).toBe(false));
    expect(loadRepositoryStatus).toHaveBeenCalledWith({ notesRoot: "/Notes" });
    expect(onFilesChanged).toHaveBeenCalledWith("/Notes");
  });

  it("recovers an accepted job when its event is lost and persisted status becomes terminal", async () => {
    vi.useFakeTimers();
    const runtime = getAppRuntime();
    const persisted = [
      dejavuStatus("attempting"),
      dejavuStatus("succeeded", null, {
        jobId: "00000000-0000-4000-8000-000000000499"
      }),
      dejavuStatus("succeeded", null, {
        repositoryId: "00000000-0000-4000-8000-000000000498"
      }),
      dejavuStatus("succeeded")
    ];
    const loadRepositoryStatus = vi.fn(async () => persisted.shift() ?? dejavuStatus("succeeded"));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        loadRepositoryStatus
      }
    });
    const onFilesChanged = vi.fn();
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });

    await act(() => result.current.run("manual"));
    await act(async () => Promise.resolve());

    expect(loadRepositoryStatus).toHaveBeenCalledTimes(1);
    expect(result.current.running).toBe(true);
    expect(vi.getTimerCount()).toBe(1);

    await act(async () => vi.advanceTimersByTimeAsync(999));
    expect(loadRepositoryStatus).toHaveBeenCalledTimes(1);

    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(loadRepositoryStatus).toHaveBeenCalledTimes(2);
    expect(result.current.running).toBe(true);
    expect(onFilesChanged).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(1);

    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(loadRepositoryStatus).toHaveBeenCalledTimes(3);
    expect(result.current.running).toBe(true);
    expect(onFilesChanged).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(1);

    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(loadRepositoryStatus).toHaveBeenCalledTimes(4);
    expect(result.current.running).toBe(false);
    expect(onFilesChanged).toHaveBeenCalledOnce();
    expect(onFilesChanged).toHaveBeenCalledWith("/Notes");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps at most one accepted-status poll outstanding while a read is in flight", async () => {
    vi.useFakeTimers();
    const runtime = getAppRuntime();
    const retry = deferred<DejavuRepositoryStatus | null>();
    const loadRepositoryStatus = vi.fn()
      .mockResolvedValueOnce(dejavuStatus("attempting"))
      .mockImplementationOnce(() => retry.promise);
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        loadRepositoryStatus
      }
    });
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const coordinator = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      })
    });
    const { result } = coordinator;

    await act(() => result.current.run("manual"));
    await act(async () => Promise.resolve());
    expect(vi.getTimerCount()).toBe(1);

    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(loadRepositoryStatus).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(0);

    await act(async () => {
      retry.resolve(dejavuStatus("attempting"));
      await retry.promise;
    });
    expect(result.current.running).toBe(true);
    expect(vi.getTimerCount()).toBe(1);
    coordinator.unmount();
  });

  it("supersedes an older accepted job once the same repository accepts a newer job", async () => {
    vi.useFakeTimers();
    const runtime = getAppRuntime();
    const newerJobId = "00000000-0000-4000-8000-000000000403";
    const loadRepositoryStatus = vi.fn()
      .mockResolvedValueOnce(dejavuStatus("attempting"))
      .mockResolvedValueOnce(dejavuStatus("succeeded", null, { jobId: newerJobId }))
      .mockResolvedValue(dejavuStatus("attempting"));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        loadRepositoryStatus
      }
    });
    mockedRunApplicationSync
      .mockResolvedValueOnce(acceptedDispatch("/Notes"))
      .mockResolvedValueOnce(acceptedDispatch("/Notes", { jobId: newerJobId }));
    const { result, rerender } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      })
    });

    await act(() => result.current.run("manual"));
    await act(async () => Promise.resolve());
    act(() => {
      rerender({
        currentDocument: configDocument("rev-2", {
          mode: "fully-manual",
          provider: "s3"
        }),
        currentRoot: "/Notes"
      });
    });
    await act(() => result.current.run("manual"));
    expect(result.current.running).toBe(true);

    await act(async () => vi.advanceTimersByTimeAsync(1_000));

    expect(loadRepositoryStatus).toHaveBeenCalledTimes(2);
    expect(result.current.running).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels accepted-status recovery when the workspace generation changes", async () => {
    vi.useFakeTimers();
    const runtime = getAppRuntime();
    const loadRepositoryStatus = vi.fn(async () => dejavuStatus("attempting"));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        loadRepositoryStatus
      }
    });
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const onFilesChanged = vi.fn();
    const { result, rerender } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });

    await act(() => result.current.run("manual"));
    await act(async () => Promise.resolve());
    expect(loadRepositoryStatus).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(1);

    act(() => {
      rerender({
        currentDocument: configDocument("rev-b", {
          mode: "fully-manual",
          provider: "s3"
        }),
        currentRoot: "/B"
      });
    });

    expect(result.current.running).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(loadRepositoryStatus).toHaveBeenCalledOnce();
    expect(onFilesChanged).not.toHaveBeenCalled();
  });

  it("cancels accepted-status recovery when a notebook switch opens a new generation", async () => {
    vi.useFakeTimers();
    const runtime = getAppRuntime();
    const loadRepositoryStatus = vi.fn(async () => dejavuStatus("attempting"));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        loadRepositoryStatus
      }
    });
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      })
    });

    await act(() => result.current.run("manual"));
    await act(async () => Promise.resolve());
    expect(result.current.running).toBe(true);
    expect(vi.getTimerCount()).toBe(1);

    await act(() => result.current.beginNotebookSwitch());

    expect(result.current.running).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(loadRepositoryStatus).toHaveBeenCalledOnce();
    await act(async () => {
      await Promise.resolve(result.current.finishNotebookSwitch());
    });
  });

  it("cancels accepted-status recovery when the coordinator unmounts", async () => {
    vi.useFakeTimers();
    const runtime = getAppRuntime();
    const loadRepositoryStatus = vi.fn(async () => dejavuStatus("attempting"));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        loadRepositoryStatus
      }
    });
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const coordinator = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      })
    });

    await act(() => coordinator.result.current.run("manual"));
    await act(async () => Promise.resolve());
    expect(loadRepositoryStatus).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(1);

    coordinator.unmount();

    expect(vi.getTimerCount()).toBe(0);
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(loadRepositoryStatus).toHaveBeenCalledOnce();
  });

  it("tracks one shared accepted job in every mounted coordinator", async () => {
    const submission = deferred<SyncDispatchResult>();
    mockedRunApplicationSync.mockImplementationOnce(() => submission.promise);
    const firstChanged = vi.fn();
    const secondChanged = vi.fn();
    const document = configDocument("rev-1", {
      mode: "fully-manual",
      provider: "s3"
    });
    const first = renderCoordinator({ document, onFilesChanged: firstChanged });
    const second = renderCoordinator({ document, onFilesChanged: secondChanged });

    let firstRun!: Promise<SyncDispatchResult | null>;
    let secondRun!: Promise<SyncDispatchResult | null>;
    act(() => {
      firstRun = first.result.current.run("manual");
      secondRun = second.result.current.run("manual");
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledOnce());
    await act(async () => {
      submission.resolve(acceptedDispatch("/Notes"));
      await Promise.all([firstRun, secondRun]);
    });

    expect(first.result.current.running).toBe(true);
    expect(second.result.current.running).toBe(true);

    await act(() => getAppRuntime().events.emit(
      "qingyu://dejavu-sync-status-changed",
      dejavuStatus("succeeded")
    ));

    await waitFor(() => expect(first.result.current.running).toBe(false));
    await waitFor(() => expect(second.result.current.running).toBe(false));
    expect(firstChanged).toHaveBeenCalledOnce();
    expect(secondChanged).toHaveBeenCalledOnce();
  });

  it("does not carry an accepted S3 job into a replacement workspace", async () => {
    const onFilesChanged = vi.fn();
    mockedRunApplicationSync.mockResolvedValueOnce(acceptedDispatch("/Notes"));
    const { result, rerender } = renderCoordinator({
      document: configDocument("rev-1", {
        mode: "fully-manual",
        provider: "s3"
      }),
      onFilesChanged
    });

    await act(() => result.current.run("manual"));
    expect(result.current.running).toBe(true);

    act(() => {
      rerender({
        currentDocument: configDocument("rev-b", {
          mode: "fully-manual",
          provider: "s3"
        }),
        currentRoot: "/B"
      });
    });

    expect(result.current.running).toBe(false);
    await act(() => getAppRuntime().events.emit(
      "qingyu://dejavu-sync-status-changed",
      dejavuStatus("succeeded")
    ));
    expect(onFilesChanged).not.toHaveBeenCalled();
  });

  it("cancels a queued old-root run and drains an already-started run before switching", async () => {
    const nativeRun = deferred<SyncDispatchResult>();
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

    nativeRun.resolve(completedDispatch("/Notes", "rev-1", "app-launch"));
    await draining;
    result.current.finishNotebookSwitch();

    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);
  });

  it("does not carry an old running count into the replacement switch generation", async () => {
    const oldRun = deferred<SyncDispatchResult>();
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
    oldRun.resolve(completedDispatch("/Notes", "rev-1", "app-launch"));
    await act(async () => {
      await draining;
      result.current.finishNotebookSwitch();
    });

    const newRun = deferred<SyncDispatchResult>();
    mockedRunApplicationSync.mockImplementationOnce(() => newRun.promise);
    let nextRun!: Promise<SyncDispatchResult | null>;
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

    newRun.resolve(completedDispatch("/Notes", "rev-1", "manual"));
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
    const nativePublication = deferred<SyncDispatchResult>();
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

    nativePublication.resolve(completedDispatch("/Notes", "rev-2", "settings-exit"));
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
    const reload = vi.fn(async () => ({ status: "loaded", ...configDocument("rev-3") } as SyncConfigLoadResult));
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
      result: {
        ...legacySyncResult("/Notes", "rev-1", "manual"),
        notebookName: "Other"
      },
      status: "completed"
    });

    let returned: SyncDispatchResult | null = completedDispatch("/placeholder", "rev", "manual");
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

  it.each(["automatic", "startup-exit"] as const)(
    "runs an eligible %s settings apply only after an exact reload",
    async (mode) => {
      const { cancelApply, syncConfig } = installRuntime();
      const reload = vi.fn(async () => ({
        status: "loaded",
        ...configDocument("rev-2", { mode })
      } as SyncConfigLoadResult));
      const { result } = renderCoordinator({
        document: configDocument("rev-1", { mode }),
        reload
      });
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
      expect(cancelApply).not.toHaveBeenCalled();
      expect((await syncConfig.loadEditing()).pendingApply).toEqual(expect.objectContaining({
        revision: "rev-2",
        sessionId: "s1",
        state: "completed",
        token: "apply-1"
      }));
    }
  );

  it.each([
    { outcome: "null", reloadResult: async () => null },
    { outcome: "rejection", reloadResult: async () => {
      throw new Error("reload failed");
    } }
  ])("cancels a settings apply when the frontend reload returns $outcome", async ({ reloadResult }) => {
    const { cancelApply, syncConfig } = installRuntime();
    const reload = vi.fn(reloadResult);
    renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));
    await act(() => emitSyncApplyRequested({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "s1",
      source: "settings-exit",
      token: "apply-reload-unavailable"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-2", sessionId: "s1" }));

    await waitFor(() => expect(reload).toHaveBeenCalled());
    await act(async () => Promise.resolve());
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    await waitFor(() => expect(cancelApply).toHaveBeenCalledWith({
      revision: "rev-2",
      sessionId: "s1",
      token: "apply-reload-unavailable"
    }));
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    expect((await syncConfig.loadEditing()).pendingApply).toEqual(expect.objectContaining({
      revision: "rev-2",
      sessionId: "s1",
      state: "completed",
      token: "apply-reload-unavailable"
    }));
  });

  it("cancels a settings apply when reload returns a different revision", async () => {
    const { cancelApply, syncConfig } = installRuntime();
    const reload = vi.fn(async () => ({
      status: "loaded",
      ...configDocument("rev-3")
    } as SyncConfigLoadResult));
    renderCoordinator({ reload });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));
    await act(() => emitSyncApplyRequested({
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "s1",
      source: "settings-exit",
      token: "apply-revision-mismatch"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-2", sessionId: "s1" }));

    await waitFor(() => expect(reload).toHaveBeenCalled());
    await act(async () => Promise.resolve());
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    await waitFor(() => expect(cancelApply).toHaveBeenCalledWith({
      revision: "rev-2",
      sessionId: "s1",
      token: "apply-revision-mismatch"
    }));
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    expect((await syncConfig.loadEditing()).pendingApply).toEqual(expect.objectContaining({
      revision: "rev-2",
      sessionId: "s1",
      state: "completed",
      token: "apply-revision-mismatch"
    }));
  });

  it("cancels queued old-root work and prevents an in-flight old result from updating the new root", async () => {
    const runA = deferred<SyncDispatchResult>();
    const changed = vi.fn();
    mockedRunApplicationSync.mockImplementationOnce(() => runA.promise);
    const { result, rerender } = renderCoordinator({ onFilesChanged: changed, primaryRoot: "/A" });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({ notesRoot: "/A" })));

    rerender({ currentDocument: configDocument("rev-b"), currentRoot: "/B" });
    expect(result.current.status).toBeNull();
    await act(async () => {
      runA.resolve(completedDispatch("/A", "rev-1", "app-launch"));
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
      document: configDocument("rev-a", { intervalSeconds: 300 }),
      primaryRoot: "/A"
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(expect.objectContaining({
      notesRoot: "/A",
      trigger: "app-launch"
    })));
    const oldTimer = setIntervalSpy.mock.calls.find(([, delay]) => delay === 300 * 1000)?.[0];
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
      currentDocument: configDocument("rev-b", { intervalSeconds: 300 }),
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

  it.each([
    ["automatic", ["app-launch", "interval", "manual", "save", "settings-exit"]],
    ["startup-exit", ["app-launch", "manual", "settings-exit"]],
    ["fully-manual", ["manual"]]
  ] as const)("limits WebDAV %s mode to its eligible triggers", async (mode, eligible) => {
    const eligibleTriggers: readonly SyncRunResult["trigger"][] = eligible;
    const { result } = renderCoordinator({ document: configDocument("rev-1", { mode }) });
    if (eligibleTriggers.includes("app-launch")) {
      await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledWith(
        expect.objectContaining({ trigger: "app-launch" })
      ));
      await waitFor(() => expect(result.current.running).toBe(false));
    } else {
      await act(async () => Promise.resolve());
      expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    }
    mockedRunApplicationSync.mockClear();

    for (const trigger of ["interval", "manual", "save", "settings-exit"] as const) {
      const callsBefore = mockedRunApplicationSync.mock.calls.length;
      await act(async () => {
        await result.current.run(trigger);
      });
      if (eligibleTriggers.includes(trigger)) {
        expect(mockedRunApplicationSync).toHaveBeenCalledTimes(callsBefore + 1);
        expect(mockedRunApplicationSync).toHaveBeenLastCalledWith(
          expect.objectContaining({ trigger })
        );
      } else {
        expect(mockedRunApplicationSync).toHaveBeenCalledTimes(callsBefore);
      }
    }
  });

  it("settles a fully-manual settings exit without starting WebDAV sync", async () => {
    const { cancelApply, syncConfig } = installRuntime();
    const reload = vi.fn(async () => ({
      status: "loaded",
      ...configDocument("rev-2", { mode: "fully-manual" })
    } as SyncConfigLoadResult));
    renderCoordinator({
      document: configDocument("rev-1", { mode: "fully-manual" }),
      reload
    });
    await act(async () => Promise.resolve());
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();

    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));
    await act(() => emitSyncApplyRequested({
      exitReason: "window-close",
      revision: "rev-2",
      sessionId: "s1",
      source: "settings-exit",
      token: "apply-manual"
    }));
    await act(() => emitSyncEditing({ active: false, revision: "rev-2", sessionId: "s1" }));

    await waitFor(() => expect(cancelApply).toHaveBeenCalledWith({
      revision: "rev-2",
      sessionId: "s1",
      token: "apply-manual"
    }));
    expect(mockedRunApplicationSync).not.toHaveBeenCalled();
    expect((await syncConfig.loadEditing()).pendingApply).toEqual(expect.objectContaining({
      revision: "rev-2",
      sessionId: "s1",
      state: "completed",
      token: "apply-manual"
    }));
  });

  it("coalesces save and manual callers by root and revision while preserving the manual result", async () => {
    const pending = deferred<SyncDispatchResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync.mockImplementationOnce(() => pending.promise);
    let manualRun: Promise<SyncDispatchResult | null> | null = null;

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

    let manualResult: SyncDispatchResult | null = null;
    await act(async () => {
      pending.resolve(completedDispatch("/Notes", "rev-1", "save"));
      manualResult = await manualRun;
    });
    expect(manualResult).toEqual({
      result: expect.objectContaining({
        notesRoot: "/Notes",
        revision: "rev-1",
        trigger: "manual"
      }),
      status: "completed"
    });
  });

  it("queues one fresh save pass when a document is saved during an active sync", async () => {
    const firstSave = deferred<SyncDispatchResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync
      .mockImplementationOnce(() => firstSave.promise)
      .mockResolvedValueOnce(completedDispatch("/Notes", "rev-1", "save"));

    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);

    await act(async () => {
      firstSave.resolve(completedDispatch("/Notes", "rev-1", "save"));
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
    const firstSave = deferred<SyncDispatchResult>();
    const trailingSave = deferred<SyncDispatchResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementationOnce(() => trailingSave.promise)
      .mockResolvedValueOnce(completedDispatch("/Notes", "rev-1", "save"));

    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await act(async () => {
      firstSave.resolve(completedDispatch("/Notes", "rev-1", "save"));
      await firstSave.promise;
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2));

    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(2);
    await act(async () => {
      trailingSave.resolve(completedDispatch("/Notes", "rev-1", "save"));
      await trailingSave.promise;
    });

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(3));
  });

  it("uses save eligibility when a save joins an active manual run", async () => {
    const manual = deferred<SyncDispatchResult>();
    const { result } = renderCoordinator();
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    mockedRunApplicationSync.mockClear();
    mockedRunApplicationSync.mockImplementationOnce(() => manual.promise);

    let manualRun!: Promise<SyncDispatchResult | null>;
    await act(() => {
      manualRun = result.current.run("manual");
    });
    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1));
    await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
    await act(() => emitSyncEditing({ active: true, revision: "rev-1", sessionId: "s1" }));

    await act(async () => {
      manual.resolve(completedDispatch("/Notes", "rev-1", "manual"));
      await manualRun;
    });

    expect(mockedRunApplicationSync).toHaveBeenCalledTimes(1);
  });

  it("does not turn a failed primary run into an automatic save retry", async () => {
    let rejectPrimary!: (error: Error) => undefined;
    const primary = new Promise<SyncDispatchResult>((_, reject) => {
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
      .mockResolvedValueOnce(completedDispatch("/Notes", "rev-1", "manual"));

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
    const retry = deferred<SyncDispatchResult>();
    installRuntime(async () => {
      throw new Error("workspace-document-membership-unavailable");
    });
    mockedRunApplicationSync
      .mockRejectedValueOnce(new Error("remote-http-error: initial failure"))
      .mockImplementationOnce(() => retry.promise)
      .mockResolvedValueOnce(completedDispatch("/B", "rev-b", "app-launch"));

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
      retry.resolve(completedDispatch("/Notes", "rev-1", "manual"));
      await retry.promise;
    });

    expect(mockedDismissAppToast).toHaveBeenCalledTimes(dismissCountBeforeRetrySettles);
  });

  it("clears an owned loading toast on workspace change without a second stale dismissal", async () => {
    const retry = deferred<SyncDispatchResult>();
    mockedRunApplicationSync
      .mockRejectedValueOnce(new Error("remote-http-error: initial failure"))
      .mockImplementationOnce(() => retry.promise)
      .mockResolvedValueOnce(completedDispatch("/B", "rev-b", "app-launch"));

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
      retry.resolve(completedDispatch("/Notes", "rev-1", "manual"));
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
      dejavuSyncAvailable: true,
      primaryRoot: "/Notes",
      reloadConfig: async () => null,
      translate: (key) => key
    }), { wrapper });

    await waitFor(() => expect(mockedRunApplicationSync).toHaveBeenCalled());
    unmount();
    await waitFor(() => expect(cleanups.length).toBeGreaterThanOrEqual(5));
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
    await waitFor(() => expect(cleanups).toHaveLength(5));
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
    await waitFor(() => expect(cleanups).toHaveLength(4));

    unmount();
    expect(cleanups.every((cleanup) => cleanup.mock.calls.length === 1)).toBe(true);

    await act(async () => {
      delayedRegistration.resolve(undefined);
      await delayedRegistration.promise;
    });
    await waitFor(() => expect(cleanups).toHaveLength(5));
    expect(cleanups.every((cleanup) => cleanup.mock.calls.length === 1)).toBe(true);
  });
});
