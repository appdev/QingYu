import { describe, expect, it, vi } from "vitest";

import { KernelProtocolError } from "./errors.ts";
import {
  createKernelEventsClient,
  type ReconnectScheduler,
  type WebSocketEvent,
  type WebSocketEventType,
  type WebSocketFactory,
  type WebSocketLike,
} from "./events.ts";

const SECRET = "websocket-native-secret";
const CONNECTION_1 = "1c399991-4574-45e9-b1dd-267f35101926";
const CONNECTION_2 = "10def922-f03b-4288-a769-441920f4cac7";
const INSTANCE_1 = "ad6f32f2-72f6-46ce-8200-2527ec98cbe5";
const WORKSPACE_1 = "f997861d-1741-438c-a533-b7e86978f3cc";

describe("Kernel events", () => {
  it("uses an exact same-origin WSS endpoint and relies on the browser session cookie", () => {
    const sockets: FakeWebSocket[] = [];
    const urls: string[] = [];
    const client = createKernelEventsClient({
      baseUrl: "https://notes.example:8443",
      auth: {
        kind: "browser-session",
        browserOrigin: "https://notes.example:8443",
        getCsrfToken: () => "unused-for-read-only-events",
      },
      webSocket: (url) => {
        urls.push(url);
        const socket = new FakeWebSocket();
        sockets.push(socket);
        return socket;
      },
    });
    const onReady = vi.fn();

    client.connect({ onReady });
    const socket = sockets[0]!;
    socket.open();
    socket.message(readyFrame(CONNECTION_1, INSTANCE_1));

    expect(urls).toEqual(["wss://notes.example:8443/api/v1/events"]);
    expect(socket.sent).toEqual([]);
    expect(onReady).toHaveBeenCalledWith(readyFrame(CONNECTION_1, INSTANCE_1));
  });

  it("uses WS for an exact same-origin HTTP browser session", () => {
    const urls: string[] = [];
    const client = createKernelEventsClient({
      baseUrl: "http://notes.example:3210",
      auth: {
        kind: "browser-session",
        browserOrigin: "http://notes.example:3210",
        getCsrfToken: () => "unused",
      },
      webSocket: (url) => {
        urls.push(url);
        return new FakeWebSocket();
      },
    });

    client.connect({});

    expect(urls).toEqual(["ws://notes.example:3210/api/v1/events"]);
  });

  it("rejects cross-scheme or cross-origin browser session event endpoints", () => {
    for (const [baseUrl, browserOrigin] of [
      ["https://api.example", "https://notes.example"],
      ["http://api.example", "http://notes.example"],
      ["http://notes.example", "https://notes.example"],
    ]) {
      expect(() => createKernelEventsClient({
        baseUrl,
        auth: {
          kind: "browser-session",
          browserOrigin,
          getCsrfToken: () => "unused",
        },
        webSocket: () => new FakeWebSocket(),
      })).toThrow();
    }
  });

  it("sends native bearer authentication as the first frame and never places it in the URL", () => {
    const harness = new EventsHarness();
    const onReady = vi.fn();
    const connection = harness.client.connect({ onReady });
    const socket = harness.sockets[0]!;

    expect(harness.urls).toEqual(["ws://127.0.0.1:6608/api/v1/events"]);
    expect(harness.urls[0]).not.toContain(SECRET);
    expect(socket.sent).toEqual([]);

    socket.open();
    expect(socket.sent.map((value) => JSON.parse(value))).toEqual([
      { type: "authenticate", protocolVersion: 1, credential: SECRET },
    ]);
    socket.message(readyFrame(CONNECTION_1, INSTANCE_1));

    expect(onReady).toHaveBeenCalledWith(readyFrame(CONNECTION_1, INSTANCE_1));
    expect(harness.getCredential).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(connection)).not.toContain(SECRET);
  });

  it("accepts canonical nil UUIDs produced by the Rust UUID wire type", () => {
    const harness = new EventsHarness();
    const onReady = vi.fn();
    harness.client.connect({ onReady });
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message(
      readyFrame(
        "00000000-0000-0000-0000-000000000000",
        "00000000-0000-0000-0000-000000000000",
      ),
    );

    expect(onReady).toHaveBeenCalledTimes(1);
  });

  it("delivers consecutive events, ignores duplicates, and treats a future sequence as stale", () => {
    const harness = new EventsHarness();
    const onEvent = vi.fn();
    const onSnapshotRequired = vi.fn();
    const onStateChange = vi.fn();
    harness.client.connect({ onEvent, onSnapshotRequired, onStateChange });
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
    onSnapshotRequired.mockClear();

    socket.message(eventFrame(CONNECTION_1, 1, "revision-1"));
    socket.message(eventFrame(CONNECTION_1, 1, "duplicate-revision"));
    socket.message(eventFrame(CONNECTION_1, 3, "future-revision"));

    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEvent).toHaveBeenCalledWith(eventFrame(CONNECTION_1, 1, "revision-1"));
    expect(onSnapshotRequired).toHaveBeenCalledWith({
      reason: "sequence-gap",
      reloadScopes: ["workspace", "documents", "settings", "sync-config", "sync-status"],
    });
    expect(onStateChange).toHaveBeenLastCalledWith("stale");
    expect(socket.closed).toEqual([{ code: 4009, reason: "snapshot reload required" }]);
  });

  it("validates protocol, first-frame readiness, and event connection identity", () => {
    const unsupported = new EventsHarness();
    const unsupportedError = vi.fn();
    unsupported.client.connect({ onError: unsupportedError });
    unsupported.sockets[0]!.open();
    unsupported.sockets[0]!.message({
      ...readyFrame(CONNECTION_1, INSTANCE_1),
      protocolVersion: 2,
    });
    expect(unsupportedError.mock.calls[0]?.[0]).toMatchObject({
      kind: "unsupported-websocket-version",
    });
    unsupported.sockets[0]!.serverClose(4001);
    expect(unsupported.scheduler.pending()).toBe(0);

    const wrongFirstFrame = new EventsHarness();
    const firstFrameError = vi.fn();
    wrongFirstFrame.client.connect({ onError: firstFrameError });
    wrongFirstFrame.sockets[0]!.open();
    wrongFirstFrame.sockets[0]!.message(eventFrame(CONNECTION_1, 1, "revision-1"));
    expect(firstFrameError.mock.calls[0]?.[0]).toMatchObject({
      kind: "invalid-websocket-frame",
    });

    const mismatch = new EventsHarness();
    const snapshot = vi.fn();
    mismatch.client.connect({ onSnapshotRequired: snapshot });
    mismatch.sockets[0]!.open();
    mismatch.sockets[0]!.message(readyFrame(CONNECTION_1, INSTANCE_1));
    snapshot.mockClear();
    mismatch.sockets[0]!.message(eventFrame(CONNECTION_2, 1, "revision-1"));
    expect(snapshot).toHaveBeenCalledWith({
      reason: "connection-mismatch",
      reloadScopes: ["workspace", "documents", "settings", "sync-config", "sync-status"],
    });
  });

  it("uses server gap scopes, reconnects with a fresh snapshot, and stops on explicit close", () => {
    const harness = new EventsHarness();
    const snapshots = vi.fn();
    const states = vi.fn();
    const connection = harness.client.connect({
      onSnapshotRequired: snapshots,
      onStateChange: states,
    });
    const first = harness.sockets[0]!;
    first.open();
    first.message(readyFrame(CONNECTION_1, INSTANCE_1));
    snapshots.mockClear();
    first.message({
      type: "gap",
      protocolVersion: 1,
      connectionId: CONNECTION_1,
      sequence: 1,
      reason: "buffer-overflow",
      reloadScopes: ["documents", "settings"],
    });

    expect(snapshots).toHaveBeenCalledWith({
      reason: "server-gap",
      reloadScopes: ["documents", "settings"],
    });
    first.serverClose(4009);
    expect(states).toHaveBeenLastCalledWith("reconnecting");
    expect(harness.scheduler.pending()).toBe(1);

    harness.scheduler.runNext();
    const second = harness.sockets[1]!;
    second.open();
    second.message(readyFrame(CONNECTION_2, INSTANCE_1));
    expect(snapshots).toHaveBeenLastCalledWith({
      reason: "reconnect",
      reloadScopes: ["workspace", "documents", "settings", "sync-config", "sync-status"],
    });
    expect(harness.getCredential).toHaveBeenCalledTimes(2);

    connection.close();
    second.serverClose(1000);
    expect(connection.state).toBe("closed");
    expect(harness.scheduler.pending()).toBe(0);
    expect(second.closed).toEqual([{ code: 1000, reason: "client closed" }]);
  });

  it("stops without reconnecting when its AbortSignal is aborted", () => {
    const harness = new EventsHarness();
    const controller = new AbortController();
    const connection = harness.client.connect({}, { signal: controller.signal });
    const socket = harness.sockets[0]!;
    socket.open();

    controller.abort();
    socket.serverClose(1000);

    expect(connection.state).toBe("closed");
    expect(harness.scheduler.pending()).toBe(0);
  });

  it("reports malformed frames without retaining their raw content", () => {
    const harness = new EventsHarness();
    const onError = vi.fn();
    harness.client.connect({ onError });
    const socket = harness.sockets[0]!;
    socket.open();
    const raw = `{ "credential": "${SECRET}"`;
    socket.messageRaw(raw);

    const error = onError.mock.calls[0]?.[0];
    expect(error).toBeInstanceOf(KernelProtocolError);
    expect(String(error)).not.toContain(raw);
    expect(JSON.stringify(error)).not.toContain(SECRET);
  });

  it("rejects malformed resources and domain events without delivering them", () => {
    const harness = new EventsHarness();
    const onEvent = vi.fn();
    const onError = vi.fn();
    harness.client.connect({ onEvent, onError });
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
    socket.message({
      ...eventFrame(CONNECTION_1, 1, "revision-1"),
      resource: {},
      event: {},
    });

    expect(onEvent).not.toHaveBeenCalled();
    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(KernelProtocolError);
    expect(onError.mock.calls[0]?.[0]).toMatchObject({
      kind: "invalid-websocket-frame",
    });
  });

  it("rejects impossible calendar timestamps in WebSocket document events", () => {
    for (const modifiedAt of ["2025-02-29T00:00:00Z", "2026-01-01T24:00:00Z"]) {
      const harness = new EventsHarness();
      const onEvent = vi.fn();
      const onError = vi.fn();
      harness.client.connect({ onEvent, onError });
      const socket = harness.sockets[0]!;
      socket.open();
      socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
      socket.message({
        ...eventFrame(CONNECTION_1, 1, "revision-1"),
        resource: { kind: "document", id: "payload.signature" },
        event: {
          type: "document-changed",
          document: {
            id: "payload.signature",
            kind: "file",
            modifiedAt,
            name: "note.md",
            parent: "",
            path: "note.md",
            revision: "revision-1",
            sizeBytes: 1,
          },
        },
      });

      expect(onEvent).not.toHaveBeenCalled();
      expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(KernelProtocolError);
    }
  });

  it("validates required error-frame fields before surfacing a server error", () => {
    const harness = new EventsHarness();
    const onError = vi.fn();
    harness.client.connect({ onError });
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message({ type: "error", protocolVersion: 1, message: "missing code" });

    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(KernelProtocolError);
    expect(onError.mock.calls[0]?.[0]).toMatchObject({
      kind: "invalid-websocket-frame",
    });
  });

  it("rejects domain events that are structurally valid but violate safe wire semantics", () => {
    const invalidEvents = [
      {
        resource: { kind: "settings" as const },
        event: {
          type: "settings-changed" as const,
          settings: {
            revision: "revision-1",
            values: [
              {
                key: "appearance.mode",
                value: { type: "integer", value: 1 },
              },
            ],
          },
        },
      },
      {
        resource: { kind: "settings" as const },
        event: {
          type: "settings-changed" as const,
          settings: {
            revision: "revision-1",
            values: [
              {
                key: "editor.fontFamily",
                value: {
                  type: "font-family",
                  value: { source: "theme", family: "unexpected" },
                },
              },
            ],
          },
        },
      },
      {
        resource: { kind: "document" as const, id: "payload.signature" },
        event: {
          type: "document-moved" as const,
          previousPath: "../outside.md",
          document: {
            id: "payload.signature",
            kind: "file" as const,
            modifiedAt: "2026-07-29T00:00:00Z",
            name: "note.md",
            parent: "",
            path: "note.md",
            revision: "revision-1",
            sizeBytes: 1,
          },
        },
      },
      {
        resource: { kind: "sync-config" as const },
        event: {
          type: "sync-config-changed" as const,
          config: {
            revision: "revision-1",
            enabled: false,
            provider: "s3" as const,
            remoteRoot: "",
            mode: "automatic" as const,
            intervalSeconds: 300,
            generateConflictDocument: true,
            configured: false,
            readiness: "disabled" as const,
            issues: [],
            webdav: {
              serverUrl: {
                value: "https://user:secret@example.com/notes?token=secret",
                redacted: false,
              },
              username: "",
              password: { present: false },
            },
            s3: {
              endpointUrl: { value: null, redacted: false },
              region: "",
              bucket: "",
              accessKeyId: { present: false },
              secretAccessKey: { present: false },
              requestTimeoutSeconds: 30,
              addressingStyle: "auto" as const,
              tlsVerification: "verify" as const,
            },
          },
        },
      },
    ];

    for (const invalidEvent of invalidEvents) {
      const harness = new EventsHarness();
      const onEvent = vi.fn();
      const onError = vi.fn();
      harness.client.connect({ onEvent, onError });
      const socket = harness.sockets[0]!;
      socket.open();
      socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
      socket.message({
        ...eventFrame(CONNECTION_1, 1, "revision-1"),
        ...invalidEvent,
      });

      expect(onEvent).not.toHaveBeenCalled();
      expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(KernelProtocolError);
    }
  });

  it("rejects unknown sync-safe error enums and the forbidden objectId field", () => {
    for (const error of [
      { code: "future_code", operation: "sync_run", provider: "s3" },
      { code: "request_failed", operation: "future_operation", provider: "s3" },
      { code: "request_failed", operation: "sync_run", provider: "s3", objectId: "secret-object" },
    ]) {
      const harness = new EventsHarness();
      const onError = vi.fn();
      const onEvent = vi.fn();
      harness.client.connect({ onError, onEvent });
      const socket = harness.sockets[0]!;
      socket.open();
      socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
      socket.message({
        ...eventFrame(CONNECTION_1, 1, "sync-1"),
        resource: { kind: "sync-status", runId: null },
        event: {
          type: "sync-status-changed",
          status: { activeRunId: null, completionState: "failed", configRevision: "sync-1", error, lastAttemptAt: null, lastSuccessfulSyncAt: null, lastTrigger: "manual", provider: "s3", summary: null },
        },
      });
      expect(onEvent).not.toHaveBeenCalled();
      expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(KernelProtocolError);
    }
  });

  it("rejects mismatched resource IDs and revisions", () => {
    for (const frame of [
      { ...eventFrame(CONNECTION_1, 1, "revision-1"), resource: { kind: "workspace", id: CONNECTION_2 } },
      { ...eventFrame(CONNECTION_1, 1, "outer-revision"), event: eventFrame(CONNECTION_1, 1, "inner-revision").event },
    ]) {
      const harness = new EventsHarness();
      const onEvent = vi.fn();
      const onError = vi.fn();
      harness.client.connect({ onEvent, onError });
      const socket = harness.sockets[0]!;
      socket.open();
      socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
      socket.message(frame);
      expect(onEvent).not.toHaveBeenCalled();
      expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(KernelProtocolError);
    }
  });

  it("rejects an empty event revision even when sync status has no config revision", () => {
    const harness = new EventsHarness();
    const onEvent = vi.fn();
    const onError = vi.fn();
    harness.client.connect({ onEvent, onError });
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
    socket.message({
      type: "event",
      protocolVersion: 1,
      connectionId: CONNECTION_1,
      sequence: 1,
      resource: { kind: "sync-status", runId: null },
      revision: "",
      event: {
        type: "sync-status-changed",
        status: {
          activeRunId: null,
          completionState: "idle",
          configRevision: null,
          error: null,
          lastAttemptAt: null,
          lastSuccessfulSyncAt: null,
          lastTrigger: null,
          provider: "s3",
          summary: null,
        },
      },
    });

    expect(onEvent).not.toHaveBeenCalled();
    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(KernelProtocolError);
  });

  it("accepts a completed sync event whose resource identifies the completed run", () => {
    const harness = new EventsHarness();
    const onEvent = vi.fn();
    const onError = vi.fn();
    harness.client.connect({ onEvent, onError });
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message(readyFrame(CONNECTION_1, INSTANCE_1));
    socket.message({
      type: "event",
      protocolVersion: 1,
      connectionId: CONNECTION_1,
      sequence: 1,
      resource: { kind: "sync-status", runId: CONNECTION_2 },
      revision: "sync-1",
      event: {
        type: "sync-status-changed",
        status: {
          activeRunId: null,
          completionState: "succeeded",
          configRevision: "sync-1",
          error: null,
          lastAttemptAt: "2026-07-29T00:00:00Z",
          lastSuccessfulSyncAt: "2026-07-29T00:00:00Z",
          lastTrigger: "manual",
          provider: "s3",
          summary: null,
        },
      },
    });

    expect(onError).not.toHaveBeenCalled();
    expect(onEvent).toHaveBeenCalledTimes(1);
  });

  it("treats an authentication close as terminal and does not reconnect", () => {
    const harness = new EventsHarness();
    const onError = vi.fn();
    const connection = harness.client.connect({ onError });
    harness.sockets[0]!.serverClose(4001);

    expect(connection.state).toBe("closed");
    expect(harness.scheduler.pending()).toBe(0);
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ kind: "server-error", frameCode: "unauthorized" });
  });

  it("ignores a queued message from a socket that has already been replaced", () => {
    const harness = new EventsHarness();
    const onEvent = vi.fn();
    harness.client.connect({ onEvent });
    const first = harness.sockets[0]!;
    first.open();
    first.message(readyFrame(CONNECTION_1, INSTANCE_1));
    const deliverStale = first.queueMessage(eventFrame(CONNECTION_1, 1, "revision-1"));
    first.serverClose(1006);
    harness.scheduler.runNext();
    const second = harness.sockets[1]!;
    second.open();
    second.message(readyFrame(CONNECTION_2, INSTANCE_1));

    deliverStale();
    expect(onEvent).not.toHaveBeenCalled();
  });
});

class EventsHarness {
  readonly sockets: FakeWebSocket[] = [];
  readonly urls: string[] = [];
  readonly scheduler = new ManualScheduler();
  readonly getCredential = vi.fn(() => SECRET);
  readonly client = createKernelEventsClient({
    baseUrl: "http://127.0.0.1:6608",
    auth: { kind: "native-bearer", getCredential: this.getCredential },
    webSocket: ((url: string) => {
      this.urls.push(url);
      const socket = new FakeWebSocket();
      this.sockets.push(socket);
      return socket;
    }) satisfies WebSocketFactory,
    scheduleReconnect: this.scheduler.schedule,
    reconnectDelayMs: 25,
  });
}

class ManualScheduler {
  readonly #scheduled: Array<{ callback: () => unknown; cancelled: boolean }> = [];

  readonly schedule: ReconnectScheduler = (callback) => {
    const item = { callback, cancelled: false };
    this.#scheduled.push(item);
    return () => {
      item.cancelled = true;
    };
  };

  pending() {
    return this.#scheduled.filter((item) => !item.cancelled).length;
  }

  runNext() {
    const item = this.#scheduled.find((candidate) => !candidate.cancelled);
    if (item === undefined) throw new Error("expected a scheduled reconnect");
    item.cancelled = true;
    item.callback();
  }
}

class FakeWebSocket implements WebSocketLike {
  readonly sent: string[] = [];
  readonly closed: Array<{ code: number | undefined; reason: string | undefined }> = [];
  readonly #listeners = new Map<WebSocketEventType, Set<(event: WebSocketEvent) => unknown>>();

  addEventListener(type: WebSocketEventType, listener: (event: WebSocketEvent) => unknown) {
    const listeners = this.#listeners.get(type) ?? new Set();
    listeners.add(listener);
    this.#listeners.set(type, listeners);
  }

  removeEventListener(type: WebSocketEventType, listener: (event: WebSocketEvent) => unknown) {
    this.#listeners.get(type)?.delete(listener);
  }

  send(data: string) {
    this.sent.push(data);
  }

  close(code?: number, reason?: string) {
    this.closed.push({ code, reason });
  }

  open() {
    this.#emit("open", {});
  }

  message(value: unknown) {
    this.messageRaw(JSON.stringify(value));
  }

  messageRaw(data: string) {
    this.#emit("message", { data });
  }

  queueMessage(value: unknown) {
    const listeners = [...(this.#listeners.get("message") ?? [])];
    const event = { data: JSON.stringify(value) };
    return () => {
      for (const listener of listeners) listener(event);
    };
  }

  serverClose(code: number) {
    this.#emit("close", { code, reason: "server close", wasClean: code === 1000 });
  }

  #emit(type: WebSocketEventType, event: WebSocketEvent) {
    for (const listener of this.#listeners.get(type) ?? []) listener(event);
  }
}

function readyFrame(connectionId: string, instanceId: string) {
  return {
    type: "ready" as const,
    protocolVersion: 1,
    connectionId,
    instanceId,
    sequence: 0,
    snapshotRequired: true as const,
  };
}

function eventFrame(connectionId: string, sequence: number, revision: string) {
  return {
    type: "event" as const,
    protocolVersion: 1,
    connectionId,
    sequence,
    resource: { kind: "workspace" as const, id: WORKSPACE_1 },
    revision,
    event: {
      type: "workspace-changed" as const,
      workspace: {
        id: WORKSPACE_1,
        generation: "generation-1",
        displayName: "Notes",
        readiness: "ready" as const,
        revision,
      },
    },
  };
}
