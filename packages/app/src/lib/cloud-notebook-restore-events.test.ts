import { waitFor } from "@testing-library/react";
import {
  listenPrimaryCloudNotebookRestoreRequested,
  primaryCloudNotebookRestoreCompletedEvent,
  primaryCloudNotebookRestoreRequestedEvent,
  requestPrimaryCloudNotebookRestore,
  type PrimaryCloudNotebookRestoreRequest
} from "./cloud-notebook-restore-events";
import {
  configureAppRuntime,
  getAppRuntime,
  resetAppRuntimeForTests,
  type AppEventsRuntime,
  type RuntimeEvent
} from "../runtime";

function validRequest(
  overrides: Partial<PrimaryCloudNotebookRestoreRequest> = {}
): PrimaryCloudNotebookRestoreRequest {
  return {
    remoteName: "Archive",
    requestId: "request-1",
    revision: "rev-2",
    ...overrides
  };
}

function createEventBus() {
  const listeners = new Map<string, Set<(event: RuntimeEvent<unknown>) => unknown>>();
  const emitted: Array<{ event: string; payload: unknown }> = [];
  const events: AppEventsRuntime = {
    isAvailable: () => true,
    emit: async <TPayload>(event: string, payload: TPayload) => {
      emitted.push({ event, payload });
      for (const listener of listeners.get(event) ?? []) {
        await listener({ payload });
      }
    },
    listen: async <TPayload>(event: string, listener: (event: RuntimeEvent<TPayload>) => unknown) => {
      const registered = listeners.get(event) ?? new Set();
      const normalizedListener = listener as (event: RuntimeEvent<unknown>) => unknown;
      registered.add(normalizedListener);
      listeners.set(event, registered);
      return () => registered.delete(normalizedListener);
    }
  };
  return {
    events,
    emit: events.emit,
    latestPayload: (event: string) => emitted.slice().reverse().find(
      (item) => item.event === event
    )?.payload,
    listenerCount: (event: string) => listeners.get(event)?.size ?? 0
  };
}

describe("primary cloud notebook restore requests", () => {
  afterEach(() => resetAppRuntimeForTests());

  it("correlates one successful restore response and ignores unrelated completions", async () => {
    const bus = createEventBus();
    configureAppRuntime({ ...getAppRuntime(), events: bus.events });
    const pending = requestPrimaryCloudNotebookRestore({
      remoteName: "Archive",
      revision: "rev-2",
      timeoutMs: 1_000
    });
    await waitFor(() => expect(
      bus.latestPayload(primaryCloudNotebookRestoreRequestedEvent)
    ).toBeDefined());
    const request = bus.latestPayload(
      primaryCloudNotebookRestoreRequestedEvent
    ) as PrimaryCloudNotebookRestoreRequest;
    await bus.emit(primaryCloudNotebookRestoreCompletedEvent, {
      requestId: "unrelated",
      succeeded: true
    });
    await bus.emit(primaryCloudNotebookRestoreCompletedEvent, {
      requestId: request.requestId,
      succeeded: true
    });
    await expect(pending).resolves.toBe(true);
  });

  it("publishes the handler result for a valid request", async () => {
    const bus = createEventBus();
    configureAppRuntime({ ...getAppRuntime(), events: bus.events });
    const handler = vi.fn(async () => true);
    await listenPrimaryCloudNotebookRestoreRequested(handler);
    const request = validRequest();

    await bus.emit(primaryCloudNotebookRestoreRequestedEvent, request);

    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith(request);
    expect(bus.latestPayload(primaryCloudNotebookRestoreCompletedEvent)).toEqual({
      requestId: request.requestId,
      succeeded: true
    });
  });

  it("publishes a safe failed completion when the primary handler rejects", async () => {
    const bus = createEventBus();
    configureAppRuntime({ ...getAppRuntime(), events: bus.events });
    const request = validRequest();
    await listenPrimaryCloudNotebookRestoreRequested(async () => {
      throw new Error("provider-secret-detail");
    });
    await bus.emit(primaryCloudNotebookRestoreRequestedEvent, request);
    expect(bus.latestPayload(primaryCloudNotebookRestoreCompletedEvent)).toEqual({
      requestId: request.requestId,
      succeeded: false
    });
    expect(JSON.stringify(
      bus.latestPayload(primaryCloudNotebookRestoreCompletedEvent)
    )).not.toContain("provider-secret-detail");
  });

  it.each([
    validRequest({ requestId: "" }),
    validRequest({ revision: "" }),
    validRequest({ remoteName: "" }),
    validRequest({ requestId: "request\0id" }),
    validRequest({ revision: "rev\0ision" }),
    validRequest({ remoteName: "Archive\0Copy" })
  ])("rejects a malformed request before calling the primary handler", async (request) => {
    const bus = createEventBus();
    configureAppRuntime({ ...getAppRuntime(), events: bus.events });
    const handler = vi.fn(async () => true);
    await listenPrimaryCloudNotebookRestoreRequested(handler);

    await bus.emit(primaryCloudNotebookRestoreRequestedEvent, request);

    expect(handler).not.toHaveBeenCalled();
    expect(bus.latestPayload(primaryCloudNotebookRestoreCompletedEvent)).toBeUndefined();
  });

  it("settles false when no primary owner responds before the supplied timeout", async () => {
    await expect(requestPrimaryCloudNotebookRestore({
      remoteName: "Archive",
      revision: "rev-2",
      timeoutMs: 1
    })).resolves.toBe(false);
  });

  it("cleans up and settles false when its caller aborts", async () => {
    const bus = createEventBus();
    configureAppRuntime({ ...getAppRuntime(), events: bus.events });
    const abortController = new AbortController();
    const pending = requestPrimaryCloudNotebookRestore({
      remoteName: "Archive",
      revision: "rev-2",
      signal: abortController.signal,
      timeoutMs: 1_000
    });
    abortController.abort();
    await expect(pending).resolves.toBe(false);
    expect(bus.listenerCount(primaryCloudNotebookRestoreCompletedEvent)).toBe(0);
  });

  it("cleans up and settles false when request delivery fails", async () => {
    const bus = createEventBus();
    configureAppRuntime({
      ...getAppRuntime(),
      events: {
        ...bus.events,
        emit: async () => {
          throw new Error("delivery failed");
        }
      }
    });

    await expect(requestPrimaryCloudNotebookRestore({
      remoteName: "Archive",
      revision: "rev-2",
      timeoutMs: 1_000
    })).resolves.toBe(false);

    expect(bus.listenerCount(primaryCloudNotebookRestoreCompletedEvent)).toBe(0);
  });
});
