import { act, renderHook, waitFor } from "@testing-library/react";
import { showAppToast } from "../lib/app-toast";
import type { DejavuRepositoryStatus, SyncConflictRecord } from "../lib/sync-config";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  getAppRuntime,
  resetAppRuntimeForTests,
  type RuntimeEvent
} from "../runtime";
import {
  dejavuSyncStatusChangedEvent,
  resetSyncConflictHistoryNoticeStateForTests,
  useSyncConflictHistory
} from "./useSyncConflictHistory";

vi.mock("../lib/app-toast", () => ({ showAppToast: vi.fn() }));

const repositoryId = "00000000-0000-4000-8000-0000000000a1";

function deferred<T>() {
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

function conflict(conflictId: string, relativePath = "folder/note.md"): SyncConflictRecord {
  return {
    conflictId,
    occurredAt: "2026-07-28T10:00:00Z",
    relativePath,
    repositoryId,
    resolution: "keep-local"
  };
}

function status(conflicts: SyncConflictRecord[]): DejavuRepositoryStatus {
  return {
    attempt: 1,
    automaticFailureCount: 0,
    conflicts,
    error: null,
    jobId: "00000000-0000-4000-8000-0000000000a2",
    lastAttemptAt: "2026-07-28T10:00:00Z",
    lastDnsRetryAt: null,
    lastSuccessfulSyncAt: "2026-07-28T10:00:00Z",
    maintenance: { lastLocalPurgeAt: null, nextLocalPurgeAt: null },
    nextScheduledAt: null,
    phase: "succeeded",
    repositoryId,
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

describe("useSyncConflictHistory", () => {
  const listeners = new Map<string, (event: RuntimeEvent<unknown>) => unknown>();
  const initial = conflict("00000000-0000-4000-8000-0000000000a3");

  beforeEach(() => {
    listeners.clear();
    resetSyncConflictHistoryNoticeStateForTests();
    vi.mocked(showAppToast).mockReset();
    const runtime = createDefaultAppRuntime();
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event, handler) => {
      listeners.set(event, handler as (event: RuntimeEvent<unknown>) => unknown);
      return () => listeners.delete(event);
    });
    runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => status([initial]));
    runtime.syncConfig.listDejavuConflictHistory = vi.fn(async () => [initial, initial]);
    runtime.syncConfig.readDejavuConflictHistory = vi.fn(async () => ({
      conflict: initial,
      local: { byteSize: 5, text: "local" },
      remote: { byteSize: 6, text: "remote" }
    }));
    configureAppRuntime(runtime);
  });

  afterEach(() => {
    resetAppRuntimeForTests();
    resetSyncConflictHistoryNoticeStateForTests();
  });

  it("loads completed conflict history without replaying old notices and matches only the active file", async () => {
    const { result } = renderHook(() => useSyncConflictHistory({
      available: true,
      notesRoot: "/notes",
      translate: (key) => key
    }));

    await waitFor(() => expect(result.current.entries).toEqual([initial]));
    expect(showAppToast).not.toHaveBeenCalled();
    await result.current.read(initial);
    expect(getAppRuntime().syncConfig.readDejavuConflictHistory).toHaveBeenCalledWith({
      conflictId: initial.conflictId,
      notesRoot: "/notes",
      repositoryId: initial.repositoryId
    });
  });

  it("keeps the persisted baseline when event subscription is unavailable", async () => {
    const runtime = getAppRuntime();
    runtime.events.listen = vi.fn(async () => {
      throw new Error("listener unavailable");
    });

    const { result } = renderHook(() => useSyncConflictHistory({
      available: true,
      notesRoot: "/notes",
      translate: (key) => key
    }));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.entries).toEqual([initial]);
    expect(showAppToast).not.toHaveBeenCalled();
  });

  it("notifies once for newly completed history without asking for a resolution", async () => {
    const { result } = renderHook(() => useSyncConflictHistory({
      available: true,
      notesRoot: "/notes",
      translate: (key) => key
    }));
    await waitFor(() => expect(listeners.has(dejavuSyncStatusChangedEvent)).toBe(true));
    const next = conflict("00000000-0000-4000-8000-0000000000a5", "new.md");
    vi.mocked(getAppRuntime().syncConfig.listDejavuConflictHistory)
      .mockResolvedValue([initial, next]);

    await act(async () => {
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial, next]) });
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial, next]) });
    });
    await waitFor(() => expect(showAppToast).toHaveBeenCalledTimes(1));
    expect(showAppToast).toHaveBeenCalledWith(expect.objectContaining({
      id: `sync-conflict-${next.conflictId}`,
      message: "sync.conflict.notice",
      status: "success"
    }));
    expect(result.current.entries).toEqual([initial, next]);
  });

  it("subscribes before the baseline read and reconciles a conflict emitted during loading", async () => {
    const firstHistory = deferred<SyncConflictRecord[]>();
    const next = conflict("00000000-0000-4000-8000-0000000000a6", "during-load.md");
    const runtime = createDefaultAppRuntime();
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event, handler) => {
      listeners.set(event, handler as (event: RuntimeEvent<unknown>) => unknown);
      return () => listeners.delete(event);
    });
    runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => status([initial]));
    runtime.syncConfig.listDejavuConflictHistory = vi.fn()
      .mockImplementationOnce(() => firstHistory.promise)
      .mockResolvedValue([initial, next]);
    configureAppRuntime(runtime);

    const { result } = renderHook(() => useSyncConflictHistory({
      available: true,
      notesRoot: "/notes",
      translate: (key) => key
    }));
    await waitFor(() => expect(runtime.syncConfig.listDejavuConflictHistory).toHaveBeenCalledTimes(1));
    expect(listeners.has(dejavuSyncStatusChangedEvent)).toBe(true);

    await act(async () => {
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial, next]) });
      firstHistory.resolve([initial]);
      await firstHistory.promise;
    });

    await waitFor(() => expect(result.current.entries).toEqual([initial, next]));
    expect(showAppToast).toHaveBeenCalledOnce();
    expect(showAppToast).toHaveBeenCalledWith(expect.objectContaining({
      id: `sync-conflict-${next.conflictId}`
    }));
  });

  it("notifies a buffered conflict already present in the first persisted history read", async () => {
    const firstHistory = deferred<SyncConflictRecord[]>();
    const next = conflict("00000000-0000-4000-8000-0000000000a7", "caught-up-during-load.md");
    const runtime = createDefaultAppRuntime();
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event, handler) => {
      listeners.set(event, handler as (event: RuntimeEvent<unknown>) => unknown);
      return () => listeners.delete(event);
    });
    runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => status([initial]));
    runtime.syncConfig.listDejavuConflictHistory = vi.fn()
      .mockImplementationOnce(() => firstHistory.promise)
      .mockResolvedValue([initial, next]);
    configureAppRuntime(runtime);

    const { result } = renderHook(() => useSyncConflictHistory({
      available: true,
      notesRoot: "/notes",
      translate: (key) => key
    }));
    await waitFor(() => expect(runtime.syncConfig.listDejavuConflictHistory).toHaveBeenCalledOnce());

    await act(async () => {
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial, next]) });
      firstHistory.resolve([initial, next]);
      await firstHistory.promise;
    });

    await waitFor(() => expect(result.current.entries).toEqual([initial, next]));
    expect(showAppToast).toHaveBeenCalledOnce();
    expect(showAppToast).toHaveBeenCalledWith(expect.objectContaining({
      id: `sync-conflict-${next.conflictId}`
    }));
  });

  it("serializes startup reconciliation before later event refreshes", async () => {
    const firstHistory = deferred<SyncConflictRecord[]>();
    const startupReconciliation = deferred<SyncConflictRecord[]>();
    const later = conflict("00000000-0000-4000-8000-0000000000a8", "later.md");
    const runtime = createDefaultAppRuntime();
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event, handler) => {
      listeners.set(event, handler as (event: RuntimeEvent<unknown>) => unknown);
      return () => listeners.delete(event);
    });
    runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => status([initial]));
    runtime.syncConfig.listDejavuConflictHistory = vi.fn()
      .mockImplementationOnce(() => firstHistory.promise)
      .mockImplementationOnce(() => startupReconciliation.promise)
      .mockResolvedValue([initial, later]);
    configureAppRuntime(runtime);

    const { result } = renderHook(() => useSyncConflictHistory({
      available: true,
      notesRoot: "/notes",
      translate: (key) => key
    }));
    await waitFor(() => expect(runtime.syncConfig.listDejavuConflictHistory).toHaveBeenCalledOnce());
    await act(async () => {
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial]) });
      firstHistory.resolve([initial]);
      await firstHistory.promise;
    });
    await waitFor(() => expect(runtime.syncConfig.listDejavuConflictHistory).toHaveBeenCalledTimes(2));

    await act(async () => {
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial, later]) });
      await Promise.resolve();
    });
    expect(runtime.syncConfig.listDejavuConflictHistory).toHaveBeenCalledTimes(2);

    await act(async () => {
      startupReconciliation.resolve([initial]);
      await startupReconciliation.promise;
    });
    await waitFor(() => expect(runtime.syncConfig.listDejavuConflictHistory).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(result.current.entries).toEqual([initial, later]));
  });

  it("stays inert when the runtime does not declare Dejavu sync", async () => {
    const runtime = getAppRuntime();
    const { result } = renderHook(() => useSyncConflictHistory({
      available: false,
      notesRoot: "/notes",
      translate: (key) => key
    }));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.entries).toEqual([]);
    expect(result.current.repositoryId).toBeNull();
    expect(runtime.events.listen).not.toHaveBeenCalled();
    expect(runtime.syncConfig.loadRepositoryStatus).not.toHaveBeenCalled();
    expect(runtime.syncConfig.listDejavuConflictHistory).not.toHaveBeenCalled();
  });
});
