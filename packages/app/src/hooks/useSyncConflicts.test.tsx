import { act, renderHook, waitFor } from "@testing-library/react";
import { showAppToast } from "../lib/app-toast";
import type { DejavuRepositoryStatus, SyncConflictRecord } from "../lib/sync-config";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type RuntimeEvent
} from "../runtime";
import {
  conflictRelativePath,
  dejavuSyncStatusChangedEvent,
  resetSyncConflictNoticeStateForTests,
  useSyncConflicts
} from "./useSyncConflicts";

vi.mock("../lib/app-toast", () => ({ showAppToast: vi.fn() }));

const repositoryId = "00000000-0000-4000-8000-0000000000a1";

function conflict(conflictId: string, relativePath = "folder/note.md"): SyncConflictRecord {
  return {
    conflictId,
    occurredAt: "2026-07-28T10:00:00Z",
    relativePath,
    repositoryId,
    resolution: null
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

describe("useSyncConflicts", () => {
  const listeners = new Map<string, (event: RuntimeEvent<unknown>) => unknown>();
  const initial = conflict("00000000-0000-4000-8000-0000000000a3");

  beforeEach(() => {
    listeners.clear();
    resetSyncConflictNoticeStateForTests();
    vi.mocked(showAppToast).mockReset();
    const runtime = createDefaultAppRuntime();
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event, handler) => {
      listeners.set(event, handler as (event: RuntimeEvent<unknown>) => unknown);
      return () => listeners.delete(event);
    });
    runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => status([initial]));
    runtime.syncConfig.listConflicts = vi.fn(async () => [initial, initial]);
    runtime.syncConfig.readConflict = vi.fn(async () => ({
      conflict: initial,
      local: { byteSize: 5, text: "local" },
      remote: { byteSize: 6, text: "remote" }
    }));
    runtime.syncConfig.resolveConflict = vi.fn(async () => ({
      jobId: "00000000-0000-4000-8000-0000000000a4",
      notesRoot: "/notes",
      repositoryId
    }));
    configureAppRuntime(runtime);
  });

  afterEach(() => resetAppRuntimeForTests());

  it("loads persisted conflicts without replaying old notices and matches only the active file", async () => {
    const { result } = renderHook(() => useSyncConflicts({
      notesRoot: "/notes",
      translate: (key) => key
    }));

    await waitFor(() => expect(result.current.conflicts).toEqual([initial]));
    expect(showAppToast).not.toHaveBeenCalled();
    expect(result.current.conflictForPath("/notes/folder/note.md")).toEqual(initial);
    expect(result.current.conflictForPath("/notes/other.md")).toBeNull();
  });

  it("notifies once for a newly emitted id and drops it after accepted resolution", async () => {
    const { result } = renderHook(() => useSyncConflicts({
      notesRoot: "/notes",
      translate: (key) => key
    }));
    await waitFor(() => expect(listeners.has(dejavuSyncStatusChangedEvent)).toBe(true));
    const next = conflict("00000000-0000-4000-8000-0000000000a5", "new.md");

    await act(async () => {
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial, next]) });
      await listeners.get(dejavuSyncStatusChangedEvent)?.({ payload: status([initial, next]) });
    });
    expect(showAppToast).toHaveBeenCalledTimes(1);
    expect(showAppToast).toHaveBeenCalledWith(expect.objectContaining({
      id: `sync-conflict-${next.conflictId}`
    }));

    await act(async () => {
      await result.current.resolve(next, { kind: "keep-local" });
    });
    expect(result.current.conflicts).toEqual([initial]);
  });

  it("normalizes Windows and POSIX roots without accepting prefix lookalikes", () => {
    expect(conflictRelativePath("C:\\Notes", "c:/notes/Folder/Note.md")).toBe("folder/note.md");
    expect(conflictRelativePath("/notes", "/notes-archive/note.md")).toBeNull();
  });
});
