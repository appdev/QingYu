import { configureAppRuntime, createDefaultAppRuntime, resetAppRuntimeForTests } from "../runtime";
import {
  emitSyncConfigChanged,
  emitSyncApplyRequested,
  emitSyncEditing,
  emitSyncRunCompleted,
  emitSyncRunRequested,
  emitSyncStatusChanged,
  listenSyncApplyRequested,
  listenSyncConfigChanged,
  listenSyncEditing,
  listenSyncRunCompleted,
  listenSyncRunRequested,
  listenSyncStatusChanged
} from "./sync-config-events";

describe("application sync events", () => {
  const emit = vi.fn();
  const listen = vi.fn();

  beforeEach(() => {
    emit.mockReset();
    listen.mockReset().mockResolvedValue(vi.fn());
    const runtime = createDefaultAppRuntime();
    configureAppRuntime({
      ...runtime,
      events: { emit, isAvailable: () => true, listen },
      syncConfig: {
        ...runtime.syncConfig,
        requestApply: async (input) => ({
          broadcasted: false,
          event: { ...input, counter: 3, state: "pending" }
        }),
        setEditing: async (input) => ({
          broadcasted: false,
          event: { ...input, counter: 2 }
        })
      }
    });
  });

  afterEach(() => resetAppRuntimeForTests());

  it("uses rootless config and editing payloads without configuration values", async () => {
    await emitSyncConfigChanged({ revision: "rev-1" });
    await emitSyncEditing({ active: true, revision: "rev-1", sessionId: "settings-1" });
    await emitSyncApplyRequested({
      exitReason: "window-close",
      revision: "rev-2",
      sessionId: "settings-1",
      source: "settings-exit",
      token: "apply-1"
    });
    await emitSyncRunRequested({
      notebookName: "Notes",
      notesRoot: "/Notes",
      requestId: "request-1",
      revision: "rev-2",
      sessionId: "settings-1",
      trigger: "manual"
    });
    await emitSyncRunCompleted({
      accepted: true,
      error: null,
      notebookName: "Notes",
      notesRoot: "/Notes",
      requestId: "request-1",
      result: null,
      revision: "rev-2",
      sessionId: "settings-1",
      trigger: "manual"
    });
    await emitSyncStatusChanged({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-2",
      status: {
        completionState: "succeeded",
        error: null,
        lastAttemptAt: "2026-07-20T00:00:00Z",
        lastSuccessfulSyncAt: "2026-07-20T00:00:01Z",
        lastTrigger: "manual",
        notebookName: "Notes",
        notesRoot: "/Notes",
        provider: "webdav",
        revision: "rev-2",
        summary: null,
        version: 1
      }
    });

    const serialized = JSON.stringify(emit.mock.calls);
    expect(serialized).not.toMatch(/projectRoot|rootPath|password|secretAccessKey/);
    expect(emit.mock.calls.map(([event]) => event)).toEqual([
      "qingyu://sync-config-changed",
      "qingyu://sync-config-editing",
      "qingyu://sync-config-apply-requested",
      "qingyu://sync-run-requested",
      "qingyu://sync-run-completed",
      "qingyu://sync-status-changed"
    ]);
  });

  it("registers every rootless application coordination listener", async () => {
    const handler = vi.fn();
    await Promise.all([
      listenSyncConfigChanged(handler),
      listenSyncEditing(handler),
      listenSyncApplyRequested(handler),
      listenSyncRunRequested(handler),
      listenSyncRunCompleted(handler),
      listenSyncStatusChanged(handler)
    ]);
    expect(listen.mock.calls.map(([event]) => event)).toEqual([
      "qingyu://sync-config-changed",
      "qingyu://sync-config-editing",
      "qingyu://sync-config-apply-requested",
      "qingyu://sync-run-requested",
      "qingyu://sync-run-completed",
      "qingyu://sync-status-changed"
    ]);
  });
});
