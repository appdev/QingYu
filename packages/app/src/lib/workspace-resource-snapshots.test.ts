import { describe, expect, it, vi } from "vitest";
import type { AppEventsRuntime, RuntimeEvent } from "../runtime";
import {
  requestWorkspaceResourceSnapshot,
  workspaceResourceSnapshotRequestEvent,
  workspaceResourceSnapshotResponseEvent,
  WorkspaceResourceSnapshotError,
  type WorkspaceResourceSnapshotRequest,
  type WorkspaceResourceSnapshotResponse
} from "./workspace-resource-snapshots";

function snapshotResponse(
  request: WorkspaceResourceSnapshotRequest,
  patch: Partial<WorkspaceResourceSnapshotResponse> = {}
): WorkspaceResourceSnapshotResponse {
  return {
    documentGeneration: 4,
    dirtyDocuments: [],
    requestId: request.requestId,
    sourceWindowLabel: request.sourceWindowLabel,
    workspaceSourcePath: request.workspaceSourcePath,
    ...patch
  };
}

function createEventsRuntime(options: {
  available?: boolean;
  onRequest?: (
    request: WorkspaceResourceSnapshotRequest,
    publish: (payload: unknown) => unknown
  ) => unknown;
} = {}) {
  const listeners = new Map<string, Set<(event: RuntimeEvent<unknown>) => unknown>>();
  const order: string[] = [];
  const cleanup = vi.fn();
  const publish = (event: string, payload: unknown) => {
    listeners.get(event)?.forEach((listener) => listener({ payload }));
  };
  const events: AppEventsRuntime = {
    emit: async (event, payload) => {
      order.push(`emit:${event}`);
      if (event === workspaceResourceSnapshotRequestEvent) {
        await options.onRequest?.(
          payload as WorkspaceResourceSnapshotRequest,
          (response) => publish(workspaceResourceSnapshotResponseEvent, response)
        );
      }
      publish(event, payload);
    },
    isAvailable: () => options.available ?? true,
    listen: async (event, listener) => {
      order.push(`listen:${event}`);
      const eventListeners = listeners.get(event) ?? new Set();
      eventListeners.add(listener as (event: RuntimeEvent<unknown>) => unknown);
      listeners.set(event, eventListeners);
      return () => {
        cleanup();
        eventListeners.delete(listener as (event: RuntimeEvent<unknown>) => unknown);
      };
    }
  };

  return { cleanup, events, order };
}

describe("workspace resource snapshot requests", () => {
  it("registers its response listener before emitting and accepts only the correlated context", async () => {
    const runtime = createEventsRuntime({
      onRequest: (request, publish) => {
        publish(snapshotResponse(request, { requestId: "other-request" }));
        publish(snapshotResponse(request, { sourceWindowLabel: "other-window" }));
        publish(snapshotResponse(request));
      }
    });

    await expect(requestWorkspaceResourceSnapshot({
      events: runtime.events,
      sourceWindowLabel: "markra-editor-2",
      workspaceSourcePath: "/notes/standalone.md"
    })).resolves.toEqual(expect.objectContaining({
      documentGeneration: 4,
      sourceWindowLabel: "markra-editor-2",
      workspaceSourcePath: "/notes/standalone.md"
    }));

    expect(runtime.order.slice(0, 2)).toEqual([
      `listen:${workspaceResourceSnapshotResponseEvent}`,
      `emit:${workspaceResourceSnapshotRequestEvent}`
    ]);
    expect(runtime.cleanup).toHaveBeenCalledTimes(1);
  });

  it("rejects a correlated malformed response and removes its listener", async () => {
    const runtime = createEventsRuntime({
      onRequest: (request, publish) => publish({
        documentGeneration: -1,
        dirtyDocuments: [],
        requestId: request.requestId,
        sourceWindowLabel: request.sourceWindowLabel,
        workspaceSourcePath: request.workspaceSourcePath
      })
    });

    await expect(requestWorkspaceResourceSnapshot({
      events: runtime.events,
      sourceWindowLabel: "main",
      workspaceSourcePath: "/vault"
    })).rejects.toMatchObject({ code: "invalid-response" });
    expect(runtime.cleanup).toHaveBeenCalledTimes(1);
  });

  it("times out cleanly when the matching editor does not answer", async () => {
    vi.useFakeTimers();
    const runtime = createEventsRuntime();
    const request = requestWorkspaceResourceSnapshot({
      events: runtime.events,
      sourceWindowLabel: "main",
      timeoutMs: 25,
      workspaceSourcePath: "/vault"
    });
    const rejection = expect(request).rejects.toMatchObject({ code: "timeout" });

    await vi.advanceTimersByTimeAsync(25);
    await rejection;
    expect(runtime.cleanup).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("rejects immediately when cross-window events are unavailable", async () => {
    const runtime = createEventsRuntime({ available: false });

    await expect(requestWorkspaceResourceSnapshot({
      events: runtime.events,
      sourceWindowLabel: "main",
      workspaceSourcePath: "/vault"
    })).rejects.toEqual(expect.objectContaining<Partial<WorkspaceResourceSnapshotError>>({
      code: "unavailable"
    }));
    expect(runtime.order).toEqual([]);
  });

  it("cancels a pending request and removes its response listener", async () => {
    const runtime = createEventsRuntime();
    const controller = new AbortController();
    const request = requestWorkspaceResourceSnapshot({
      events: runtime.events,
      signal: controller.signal,
      sourceWindowLabel: "main",
      workspaceSourcePath: "/vault"
    });
    const rejection = expect(request).rejects.toMatchObject({ name: "AbortError" });
    await Promise.resolve();
    await Promise.resolve();

    controller.abort();

    await rejection;
    expect(runtime.cleanup).toHaveBeenCalledTimes(1);
  });
});
