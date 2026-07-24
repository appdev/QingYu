import { act, renderHook, waitFor } from "@testing-library/react";
import type {
  SyncConfigDocument,
  SyncRunResult
} from "../lib/sync-config";
import {
  syncRunCompletedEvent,
  syncRunRequestedEvent,
  type SyncRunCompletedPayload,
  type SyncRunRequestedPayload
} from "../lib/sync-config-events";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppRuntime,
  type RuntimeEvent
} from "../runtime";
import { useSyncSettingsSession } from "./useSyncSettingsSession";

type Deferred<T> = {
  promise: Promise<T>;
  reject: (reason?: unknown) => void;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let reject!: Deferred<T>["reject"];
  let resolve!: Deferred<T>["resolve"];
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function syncDocument(revision: string, remoteRoot = "qingyu"): SyncConfigDocument {
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
      webdav: {
        password: "",
        serverUrl: "https://dav.example.test",
        username: ""
      }
    },
    configured: true,
    issues: [],
    readiness: "ready",
    revision
  };
}

function syncResult(notesRoot: string, revision: string): SyncRunResult {
  return {
    notebookName: notesRoot.split(/[\\/]/).at(-1) ?? "",
    notesRoot,
    provider: "webdav",
    revision,
    summary: {
      bytesDownloaded: 0,
      bytesUploaded: 0,
      conflictFiles: 0,
      downloadedFiles: 0,
      scannedFiles: 1,
      skippedFiles: 0,
      uploadedFiles: 0
    },
    trigger: "manual"
  };
}

type EventHarness = {
  emit: <TPayload>(event: string, payload: TPayload) => Promise<unknown>;
  emitted: Array<{ event: string; payload: unknown }>;
  listeners: Map<string, Set<(event: RuntimeEvent<unknown>) => unknown>>;
};

function eventHarness(): EventHarness {
  const emitted: EventHarness["emitted"] = [];
  const listeners: EventHarness["listeners"] = new Map();
  return {
    emit: async (event, payload) => {
      emitted.push({ event, payload });
      for (const listener of listeners.get(event) ?? []) listener({ payload });
    },
    emitted,
    listeners
  };
}

function configureSessionRuntime({
  events = eventHarness(),
  syncConfig = {}
}: {
  events?: EventHarness;
  syncConfig?: Partial<AppRuntime["syncConfig"]>;
} = {}) {
  const runtime = createDefaultAppRuntime();
  configureAppRuntime({
    ...runtime,
    events: {
      emit: events.emit,
      isAvailable: () => true,
      listen: async (event, listener) => {
        const registered = events.listeners.get(event) ?? new Set();
        registered.add(listener as (event: RuntimeEvent<unknown>) => unknown);
        events.listeners.set(event, registered);
        return () => registered.delete(listener as (event: RuntimeEvent<unknown>) => unknown);
      }
    },
    syncConfig: {
      ...runtime.syncConfig,
      load: async () => ({ ...syncDocument("rev-1"), status: "loaded" as const }),
      ...syncConfig
    }
  });
  return events;
}

function requestedRuns(events: EventHarness) {
  return events.emitted
    .filter(({ event }) => event === syncRunRequestedEvent)
    .map(({ payload }) => payload as SyncRunRequestedPayload);
}

function completion(request: SyncRunRequestedPayload, overrides: Partial<SyncRunCompletedPayload> = {}) {
  return {
    accepted: true,
    error: null,
    notebookName: request.notebookName,
    notesRoot: request.notesRoot,
    requestId: request.requestId,
    result: syncResult(request.notesRoot, request.revision),
    revision: request.revision,
    sessionId: request.sessionId,
    trigger: "manual" as const,
    ...overrides
  };
}

describe("useSyncSettingsSession", () => {
  beforeEach(() => resetAppRuntimeForTests());
  afterEach(() => resetAppRuntimeForTests());

  it("persists each field immediately and requests one apply only when the session ends", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const patch = vi.fn(async () => syncDocument("rev-2", "qingyu/team"));
    const requestApply = vi.fn(defaultRuntime.syncConfig.requestApply);
    configureAppRuntime({
      ...defaultRuntime,
      syncConfig: {
        ...defaultRuntime.syncConfig,
        load: vi.fn(async () => ({ ...syncDocument("rev-1"), status: "loaded" as const })),
        patch,
        requestApply,
        setEditing: vi.fn(defaultRuntime.syncConfig.setEditing)
      }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));

    await act(async () => result.current.begin());
    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/team" }));

    expect(patch).toHaveBeenCalledWith({
      expectedRevision: "rev-1",
      patch: { field: "remoteRoot", value: "qingyu/team" }
    });
    expect(requestApply).not.toHaveBeenCalled();

    await act(async () => result.current.end("category-leave"));
    await waitFor(() => expect(requestApply).toHaveBeenCalledTimes(1));
    expect(requestApply).toHaveBeenCalledWith(expect.objectContaining({
      exitReason: "category-leave",
      revision: "rev-2",
      source: "settings-exit"
    }));
  });

  it("hands a saved draft to cloud catalog discovery without starting synchronization", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const requestApply = vi.fn(defaultRuntime.syncConfig.requestApply);
    const setEditing = vi.fn(defaultRuntime.syncConfig.setEditing);
    configureSessionRuntime({
      syncConfig: {
        patch: async () => syncDocument("rev-2", "qingyu/team"),
        requestApply,
        setEditing
      }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));

    await act(async () => result.current.begin());
    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/team" }));
    await act(async () => result.current.end("catalog-handoff"));

    expect(requestApply).not.toHaveBeenCalled();
    expect(setEditing).toHaveBeenLastCalledWith(expect.objectContaining({
      active: false,
      revision: "rev-2"
    }));
    expect(result.current.sessionId).toBeNull();
  });

  it("deduplicates concurrent begin calls into one editing session", async () => {
    const pendingLoad = deferred<ReturnType<typeof syncDocument> & { status: "loaded" }>();
    const load = vi.fn(() => pendingLoad.promise);
    const setEditing = vi.fn(createDefaultAppRuntime().syncConfig.setEditing);
    configureSessionRuntime({ syncConfig: { load, setEditing } });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));

    let first!: Promise<unknown>;
    let second!: Promise<unknown>;
    act(() => {
      first = result.current.begin();
      second = result.current.begin();
    });
    expect(load).toHaveBeenCalledTimes(1);

    pendingLoad.resolve({ ...syncDocument("rev-1"), status: "loaded" });
    await act(async () => Promise.all([first, second]));

    expect(setEditing).toHaveBeenCalledTimes(1);
    expect(setEditing).toHaveBeenCalledWith(expect.objectContaining({ active: true }));
  });

  it("serializes immediate writes against the revision returned by the previous write", async () => {
    const firstWrite = deferred<SyncConfigDocument>();
    const secondWrite = deferred<SyncConfigDocument>();
    const patch = vi.fn()
      .mockImplementationOnce(() => firstWrite.promise)
      .mockImplementationOnce(() => secondWrite.promise);
    configureSessionRuntime({ syncConfig: { patch } });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));
    await act(async () => result.current.begin());

    let first!: Promise<unknown>;
    let second!: Promise<unknown>;
    act(() => {
      first = result.current.patch({ field: "remoteRoot", value: "qingyu/a" });
      second = result.current.patch({ field: "remoteRoot", value: "qingyu/b" });
    });
    await waitFor(() => expect(patch).toHaveBeenCalledTimes(1));
    expect(patch).toHaveBeenNthCalledWith(1, {
      expectedRevision: "rev-1",
      patch: { field: "remoteRoot", value: "qingyu/a" }
    });

    firstWrite.resolve(syncDocument("rev-2", "qingyu/a"));
    await waitFor(() => expect(patch).toHaveBeenCalledTimes(2));
    expect(patch).toHaveBeenNthCalledWith(2, {
      expectedRevision: "rev-2",
      patch: { field: "remoteRoot", value: "qingyu/b" }
    });
    secondWrite.resolve(syncDocument("rev-3", "qingyu/b"));
    await act(async () => Promise.all([first, second]));
    expect(result.current.loadResult).toEqual(expect.objectContaining({ revision: "rev-3" }));
  });

  it("waits for queued writes and applies once when concurrent exit paths end the session", async () => {
    const write = deferred<SyncConfigDocument>();
    const defaultRuntime = createDefaultAppRuntime();
    const requestApply = vi.fn(defaultRuntime.syncConfig.requestApply);
    configureSessionRuntime({
      syncConfig: {
        patch: vi.fn(() => write.promise),
        requestApply,
        setEditing: defaultRuntime.syncConfig.setEditing
      }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));
    await act(async () => result.current.begin());

    let save!: Promise<unknown>;
    let categoryLeave!: Promise<unknown>;
    let windowClose!: Promise<unknown>;
    act(() => {
      save = result.current.patch({ field: "remoteRoot", value: "qingyu/team" });
      categoryLeave = result.current.end("category-leave");
      windowClose = result.current.end("window-close");
    });
    expect(requestApply).not.toHaveBeenCalled();

    write.resolve(syncDocument("rev-2", "qingyu/team"));
    await act(async () => Promise.all([save, categoryLeave, windowClose]));
    expect(requestApply).toHaveBeenCalledTimes(1);
    expect(requestApply).toHaveBeenCalledWith(expect.objectContaining({
      exitReason: "category-leave",
      revision: "rev-2"
    }));
  });

  it("retries a failed exit apply with the same idempotency token", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const requestApply = vi.fn()
      .mockRejectedValueOnce(new Error("temporary apply failure"))
      .mockImplementation(defaultRuntime.syncConfig.requestApply);
    configureSessionRuntime({
      syncConfig: {
        patch: async () => syncDocument("rev-2", "qingyu/team"),
        requestApply,
        setEditing: defaultRuntime.syncConfig.setEditing
      }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));
    await act(async () => result.current.begin());
    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/team" }));

    await act(async () => {
      await expect(result.current.end("category-leave")).rejects.toThrow("temporary apply failure");
    });
    const firstToken = requestApply.mock.calls[0]?.[0].token;
    await act(async () => result.current.end("window-close"));

    expect(requestApply).toHaveBeenCalledTimes(2);
    expect(requestApply.mock.calls[1]?.[0].token).toBe(firstToken);
  });

  it("blocks exit until every failed write key is retried successfully", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const setEditing = vi.fn(defaultRuntime.syncConfig.setEditing);
    const patch = vi.fn()
      .mockRejectedValueOnce(new Error("remote root save failed"))
      .mockResolvedValueOnce(syncDocument("rev-2"))
      .mockResolvedValueOnce(syncDocument("rev-3", "qingyu/retried"));
    configureSessionRuntime({
      syncConfig: {
        patch,
        requestApply: defaultRuntime.syncConfig.requestApply,
        setEditing
      }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));
    await act(async () => result.current.begin());

    await act(async () => {
      await expect(result.current.patch({ field: "remoteRoot", value: "qingyu/broken" }))
        .rejects.toThrow("remote root save failed");
    });
    await act(async () => result.current.patch({ field: "provider", value: "s3" }));
    await act(async () => {
      await expect(result.current.end("category-leave")).rejects.toThrow("remote root save failed");
    });
    expect(result.current.sessionId).not.toBeNull();
    expect(setEditing).not.toHaveBeenCalledWith(expect.objectContaining({ active: false }));

    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/retried" }));
    await act(async () => result.current.end("category-leave"));

    expect(result.current.sessionId).toBeNull();
    expect(setEditing).toHaveBeenLastCalledWith(expect.objectContaining({ active: false }));
  });

  it("uses a new apply token after edits advance the revision following an exit failure", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const requestApply = vi.fn()
      .mockRejectedValueOnce(new Error("first apply failed"))
      .mockRejectedValueOnce(new Error("second apply failed"))
      .mockImplementation(defaultRuntime.syncConfig.requestApply);
    const patch = vi.fn()
      .mockResolvedValueOnce(syncDocument("rev-2", "qingyu/a"))
      .mockResolvedValueOnce(syncDocument("rev-3", "qingyu/b"));
    configureSessionRuntime({
      syncConfig: {
        patch,
        requestApply,
        setEditing: defaultRuntime.syncConfig.setEditing
      }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));
    await act(async () => result.current.begin());
    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/a" }));

    await act(async () => {
      await expect(result.current.end("category-leave")).rejects.toThrow("first apply failed");
    });
    const firstToken = requestApply.mock.calls[0]?.[0].token;

    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/b" }));
    await act(async () => {
      await expect(result.current.end("window-close")).rejects.toThrow("second apply failed");
    });
    const secondToken = requestApply.mock.calls[1]?.[0].token;
    expect(secondToken).not.toBe(firstToken);

    await act(async () => result.current.end("window-close"));
    expect(requestApply.mock.calls[2]?.[0].token).toBe(secondToken);
    expect(requestApply.mock.calls[2]?.[0].revision).toBe("rev-3");
  });

  it("releases native editing state once when the component unmounts", async () => {
    const setEditing = vi.fn(createDefaultAppRuntime().syncConfig.setEditing);
    configureSessionRuntime({ syncConfig: { setEditing } });
    const { result, unmount } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));
    await act(async () => result.current.begin());

    unmount();

    await waitFor(() => expect(setEditing).toHaveBeenCalledTimes(2));
    expect(setEditing).toHaveBeenLastCalledWith(expect.objectContaining({ active: false }));
  });

  it("does not clear dirty state for a manual completion from a stale root or revision", async () => {
    const events = configureSessionRuntime({
      syncConfig: {
        patch: vi.fn()
          .mockResolvedValueOnce(syncDocument("rev-2", "qingyu/a"))
          .mockResolvedValueOnce(syncDocument("rev-3", "qingyu/b"))
          .mockResolvedValueOnce(syncDocument("rev-4", "qingyu/b"))
      }
    });
    const { result, rerender } = renderHook(
      ({ primaryRoot }) => useSyncSettingsSession({ primaryRoot }),
      { initialProps: { primaryRoot: "/Notes/A" } }
    );
    await waitFor(() => expect(events.listeners.has(syncRunCompletedEvent)).toBe(true));
    await act(async () => result.current.begin());
    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/a" }));
    await act(async () => result.current.runImmediate());
    const rootRequest = requestedRuns(events)[0]!;

    rerender({ primaryRoot: "/Notes/B" });
    await act(async () => events.emit(syncRunCompletedEvent, completion(rootRequest)));
    expect(result.current.dirty).toBe(true);

    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/b" }));
    await act(async () => result.current.runImmediate());
    const revisionRequest = requestedRuns(events)[1]!;
    await act(async () => result.current.patch({ field: "enabled", value: false }));
    await act(async () => events.emit(syncRunCompletedEvent, completion(revisionRequest)));
    expect(result.current.dirty).toBe(true);
  });

  it("clears dirty state only after the current root and revision complete successfully", async () => {
    const events = configureSessionRuntime({
      syncConfig: { patch: async () => syncDocument("rev-2", "qingyu/team") }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: "/Notes" }));
    await waitFor(() => expect(events.listeners.has(syncRunCompletedEvent)).toBe(true));
    await act(async () => result.current.begin());
    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/team" }));
    await act(async () => result.current.runImmediate());

    const request = requestedRuns(events)[0]!;
    await act(async () => events.emit(syncRunCompletedEvent, completion(request)));

    expect(result.current.dirty).toBe(false);
  });

  it("loads and edits application sync configuration even without a primary workspace", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const load = vi.fn(async () => ({ ...syncDocument("rev-1"), status: "loaded" as const }));
    configureAppRuntime({
      ...defaultRuntime,
      syncConfig: {
        ...defaultRuntime.syncConfig,
        load,
        patch: async () => syncDocument("rev-2", "qingyu/team")
      }
    });
    const { result } = renderHook(() => useSyncSettingsSession({ primaryRoot: null }));

    await act(async () => result.current.begin());

    expect(load).toHaveBeenCalledTimes(1);
    expect(result.current.loadResult?.status).toBe("loaded");
    await act(async () => result.current.patch({ field: "remoteRoot", value: "qingyu/team" }));
    await expect(result.current.runImmediate()).rejects.toThrow("primary notes workspace");
    expect(load).toHaveBeenCalledTimes(1);
  });
});
