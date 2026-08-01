import type { NativeKernelBootstrap } from "../kernel-bootstrap";
import type {
  ReconnectScheduler,
  WebSocketEvent,
  WebSocketEventType,
  WebSocketFactory,
  WebSocketLike
} from "@markra/kernel-client";
import { describe, expect, it, vi } from "vitest";

import {
  createDesktopKernelEventsAdapter,
  type DesktopKernelEventsAdapter,
  type DesktopKernelDomainInvalidation,
  type DesktopKernelDomainScope,
  type DesktopKernelEventsStateNotice
} from "./kernel-events";

const INSTANCE_A = "ad6f32f2-72f6-46ce-8200-2527ec98cbe5";
const INSTANCE_B = "6da0def1-9ea1-4b10-b581-d35a824c7030";
const CONNECTION_A = "1c399991-4574-45e9-b1dd-267f35101926";
const CONNECTION_B = "10def922-f03b-4288-a769-441920f4cac7";
const WORKSPACE_ID = "f997861d-1741-438c-a533-b7e86978f3cc";
const SECRET = "desktop-kernel-events-secret";
const ALL_SCOPES = [
  "workspace",
  "documents",
  "settings",
  "app-config",
  "sync-config",
  "sync-status"
] as const satisfies readonly DesktopKernelDomainScope[];

describe("Desktop Kernel domain events adapter", () => {
  it("turns ready snapshots and domain frames into identity-bound invalidations", () => {
    const harness = new AdapterHarness();
    harness.adapter.replaceConnection(bootstrap(INSTANCE_A, "7"));
    const socket = harness.sockets[0]!;

    socket.open();
    socket.message(readyFrame(CONNECTION_A, INSTANCE_A));
    socket.message(workspaceEvent(CONNECTION_A, 1, "workspace-revision-1"));

    expect(harness.invalidations).toEqual([
      {
        kind: "snapshot-required",
        instanceId: INSTANCE_A,
        generation: "7",
        reason: "ready",
        scopes: ALL_SCOPES
      },
      {
        kind: "event",
        instanceId: INSTANCE_A,
        generation: "7",
        scope: "workspace",
        frame: workspaceEvent(CONNECTION_A, 1, "workspace-revision-1")
      }
    ]);
    expect(harness.adapter.identity).toEqual({ instanceId: INSTANCE_A, generation: "7" });
  });

  it("maps sequence gaps to typed snapshot invalidations", () => {
    const harness = new AdapterHarness();
    harness.adapter.replaceConnection(bootstrap(INSTANCE_A, "8"));
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message(readyFrame(CONNECTION_A, INSTANCE_A));
    harness.invalidations.length = 0;

    socket.message(workspaceEvent(CONNECTION_A, 2, "workspace-revision-2"));

    expect(harness.invalidations).toEqual([
      {
        kind: "snapshot-required",
        instanceId: INSTANCE_A,
        generation: "8",
        reason: "sequence-gap",
        scopes: ALL_SCOPES
      }
    ]);
    expect(socket.closed).toEqual([
      { code: 4009, reason: "snapshot reload required" }
    ]);
  });

  it("maps every frozen Kernel event family without inventing a resource scope", () => {
    const harness = new AdapterHarness();
    harness.adapter.replaceConnection(bootstrap(INSTANCE_A, "8"));
    const socket = harness.sockets[0]!;
    socket.open();
    socket.message(readyFrame(CONNECTION_A, INSTANCE_A));
    harness.invalidations.length = 0;

    for (const frame of domainEventFrames(CONNECTION_A)) socket.message(frame);

    expect(harness.invalidations.map((invalidation) => (
      invalidation.kind === "event" ? invalidation.scope : "snapshot"
    ))).toEqual([
      "workspace",
      "documents",
      "settings",
      "app-config",
      "sync-config",
      "sync-status"
    ]);
  });

  it("reconnects the current identity with a fresh snapshot", () => {
    const harness = new AdapterHarness();
    harness.adapter.replaceConnection(bootstrap(INSTANCE_A, "9"));
    const first = harness.sockets[0]!;
    first.open();
    first.message(readyFrame(CONNECTION_A, INSTANCE_A));
    harness.invalidations.length = 0;

    first.serverClose(1006);
    expect(harness.states.at(-1)).toEqual({
      instanceId: INSTANCE_A,
      generation: "9",
      state: "reconnecting"
    });
    harness.scheduler.runNext();
    const second = harness.sockets[1]!;
    second.open();
    second.message(readyFrame(CONNECTION_B, INSTANCE_A));

    expect(harness.invalidations).toEqual([
      {
        kind: "snapshot-required",
        instanceId: INSTANCE_A,
        generation: "9",
        reason: "reconnect",
        scopes: ALL_SCOPES
      }
    ]);
  });

  it.each([
    [INSTANCE_A, "10", INSTANCE_A, "11"],
    [INSTANCE_A, "10", INSTANCE_B, "10"]
  ])("cancels stale callbacks when identity changes from %s/%s to %s/%s", (
    firstInstance,
    firstGeneration,
    nextInstance,
    nextGeneration
  ) => {
    const harness = new AdapterHarness();
    const firstRelease = vi.fn(() => undefined);
    const nextRelease = vi.fn(() => undefined);
    harness.adapter.replaceConnection(bootstrap(firstInstance, firstGeneration, {
      release: firstRelease
    }));
    const first = harness.sockets[0]!;
    first.open();
    first.message(readyFrame(CONNECTION_A, firstInstance));
    const deliverStale = first.queueMessage(
      workspaceEvent(CONNECTION_A, 1, "stale-revision")
    );
    harness.invalidations.length = 0;
    harness.states.length = 0;

    harness.adapter.replaceConnection(bootstrap(nextInstance, nextGeneration, {
      release: nextRelease
    }));
    const second = harness.sockets[1]!;
    deliverStale();
    second.open();
    second.message(readyFrame(CONNECTION_B, nextInstance));

    expect(first.closed).toEqual([{ code: 1000, reason: "client closed" }]);
    expect(firstRelease).toHaveBeenCalledTimes(1);
    expect(harness.invalidations).toEqual([
      {
        kind: "snapshot-required",
        instanceId: nextInstance,
        generation: nextGeneration,
        reason: "ready",
        scopes: ALL_SCOPES
      }
    ]);
    expect(harness.states.every((notice) => (
      notice.instanceId === nextInstance && notice.generation === nextGeneration
    ))).toBe(true);
    harness.adapter.close();
    expect(nextRelease).toHaveBeenCalledTimes(1);
  });

  it("keeps the live stream when bootstrap identity is unchanged", () => {
    const harness = new AdapterHarness();
    const adoptedRelease = vi.fn(() => undefined);
    const duplicateRelease = vi.fn(() => undefined);
    const adopted = bootstrap(INSTANCE_A, "11", {
      release: adoptedRelease
    });
    harness.adapter.replaceConnection(adopted);
    const first = harness.sockets[0]!;

    harness.adapter.replaceConnection(adopted);
    harness.adapter.replaceConnection(bootstrap(INSTANCE_A, "11", {
      release: duplicateRelease
    }));

    expect(harness.sockets).toHaveLength(1);
    expect(first.closed).toEqual([]);
    expect(duplicateRelease).toHaveBeenCalledTimes(1);
    expect(adoptedRelease).not.toHaveBeenCalled();
    harness.adapter.close();
    harness.adapter.close();
    expect(adoptedRelease).toHaveBeenCalledTimes(1);
  });

  it("closes a ready stream whose Kernel instance disagrees with bootstrap", () => {
    const harness = new AdapterHarness();
    const release = vi.fn(() => undefined);
    harness.adapter.replaceConnection(bootstrap(INSTANCE_A, "12", { release }));
    const socket = harness.sockets[0]!;
    socket.open();

    socket.message(readyFrame(CONNECTION_A, INSTANCE_B));

    expect(socket.closed).toEqual([{ code: 1000, reason: "client closed" }]);
    expect(harness.adapter.identity).toBeNull();
    expect(harness.states).toEqual([
      { instanceId: INSTANCE_A, generation: "12", state: "connecting" },
      { instanceId: INSTANCE_A, generation: "12", state: "closed" }
    ]);
    expect(release).toHaveBeenCalledTimes(1);
    expect(harness.invalidations).toEqual([
      {
        kind: "snapshot-required",
        instanceId: INSTANCE_A,
        generation: "12",
        reason: "instance-mismatch",
        scopes: ALL_SCOPES
      }
    ]);
  });

  it("finishes mismatch cleanup when the invalidation consumer throws", () => {
    const states: DesktopKernelEventsStateNotice[] = [];
    const sockets: FakeWebSocket[] = [];
    const release = vi.fn(() => undefined);
    const onInvalidation = vi.fn(() => {
      throw new Error(SECRET);
    });
    const adapter = createDesktopKernelEventsAdapter({
      onInvalidation,
      onStateChange: (notice) => states.push(notice),
      webSocket: () => {
        const socket = new FakeWebSocket();
        sockets.push(socket);
        return socket;
      }
    });
    adapter.replaceConnection(bootstrap(INSTANCE_A, "12", { release }));
    const socket = sockets[0]!;
    socket.open();

    expect(() => socket.message(readyFrame(CONNECTION_A, INSTANCE_B))).not.toThrow();

    expect(onInvalidation).toHaveBeenCalledTimes(1);
    expect(socket.closed).toEqual([{ code: 1000, reason: "client closed" }]);
    expect(adapter.identity).toBeNull();
    expect(release).toHaveBeenCalledTimes(1);
    expect(states).toEqual([
      { instanceId: INSTANCE_A, generation: "12", state: "connecting" },
      { instanceId: INSTANCE_A, generation: "12", state: "closed" }
    ]);
  });

  it.each([
    ["malformed", `{ "credential": "${SECRET}"`, 4002],
    ["authentication", JSON.stringify({
      type: "error",
      protocolVersion: 1,
      code: "unauthorized",
      message: "Authentication is required."
    }), 4001]
  ])("finishes %s terminal cleanup when the error consumer throws", (
    _case,
    frame,
    closeCode
  ) => {
    const states: DesktopKernelEventsStateNotice[] = [];
    const sockets: FakeWebSocket[] = [];
    const release = vi.fn(() => undefined);
    const onError = vi.fn(() => {
      throw new Error(SECRET);
    });
    const adapter = createDesktopKernelEventsAdapter({
      onInvalidation: vi.fn(),
      onError,
      onStateChange: (notice) => states.push(notice),
      webSocket: () => {
        const socket = new FakeWebSocket();
        sockets.push(socket);
        return socket;
      }
    });
    adapter.replaceConnection(bootstrap(INSTANCE_A, "13", { release }));
    const socket = sockets[0]!;
    socket.open();

    expect(() => socket.messageRaw(frame)).not.toThrow();
    expect(socket.closed).toEqual([{ code: closeCode, reason: "event protocol failed" }]);
    socket.serverClose(closeCode);

    expect(onError).toHaveBeenCalledTimes(1);
    expect(adapter.identity).toBeNull();
    expect(release).toHaveBeenCalledTimes(1);
    expect(states).toEqual([
      { instanceId: INSTANCE_A, generation: "13", state: "connecting" },
      { instanceId: INSTANCE_A, generation: "13", state: "closed" }
    ]);
    adapter.close();
    expect(release).toHaveBeenCalledTimes(1);
  });

  it("isolates state consumer failures from connect, ready, and close cleanup", () => {
    const sockets: FakeWebSocket[] = [];
    const invalidations: DesktopKernelDomainInvalidation[] = [];
    const release = vi.fn(() => undefined);
    const onStateChange = vi.fn(() => {
      throw new Error(SECRET);
    });
    const adapter = createDesktopKernelEventsAdapter({
      onInvalidation: (invalidation) => invalidations.push(invalidation),
      onStateChange,
      webSocket: () => {
        const socket = new FakeWebSocket();
        sockets.push(socket);
        return socket;
      }
    });

    expect(() => adapter.replaceConnection(bootstrap(INSTANCE_A, "14", {
      release
    }))).not.toThrow();
    const socket = sockets[0]!;
    socket.open();
    expect(() => socket.message(readyFrame(CONNECTION_A, INSTANCE_A))).not.toThrow();

    expect(adapter.identity).toEqual({ instanceId: INSTANCE_A, generation: "14" });
    expect(invalidations).toHaveLength(1);
    expect(() => adapter.close()).not.toThrow();
    expect(socket.closed).toEqual([{ code: 1000, reason: "client closed" }]);
    expect(adapter.identity).toBeNull();
    expect(release).toHaveBeenCalledTimes(1);
    expect(onStateChange).toHaveBeenCalledTimes(3);
  });

  it("disconnects explicitly and cancels a pending reconnect", () => {
    const harness = new AdapterHarness();
    const release = vi.fn(() => undefined);
    harness.adapter.replaceConnection(bootstrap(INSTANCE_A, "13", { release }));
    harness.sockets[0]!.serverClose(1006);
    expect(harness.scheduler.pending()).toBe(1);

    harness.adapter.close();
    harness.adapter.close();

    expect(harness.scheduler.pending()).toBe(0);
    expect(harness.adapter.identity).toBeNull();
    expect(release).toHaveBeenCalledTimes(1);
  });

  it("allows a terminal state callback to install the successor identity", () => {
    const sockets: FakeWebSocket[] = [];
    const firstRelease = vi.fn(() => undefined);
    const nextRelease = vi.fn(() => undefined);
    let adapter: DesktopKernelEventsAdapter | undefined;
    adapter = createDesktopKernelEventsAdapter({
      onInvalidation: vi.fn(),
      onStateChange: (notice) => {
        if (notice.state === "closed") {
          adapter?.replaceConnection(bootstrap(INSTANCE_B, "15", {
            release: nextRelease
          }));
        }
      },
      webSocket: () => {
        const socket = new FakeWebSocket();
        sockets.push(socket);
        return socket;
      }
    });
    adapter.replaceConnection(bootstrap(INSTANCE_A, "14", {
      release: firstRelease
    }));

    sockets[0]!.serverClose(4001);

    expect(sockets).toHaveLength(2);
    expect(adapter.identity).toEqual({ instanceId: INSTANCE_B, generation: "15" });
    expect(firstRelease).toHaveBeenCalledTimes(1);
    adapter.close();
    expect(nextRelease).toHaveBeenCalledTimes(1);
  });

  it("releases ownership when event client initialization rejects", () => {
    const release = vi.fn(() => undefined);
    const adapter = createDesktopKernelEventsAdapter({
      onInvalidation: vi.fn(),
      webSocket: () => new FakeWebSocket()
    });

    expect(() => adapter.replaceConnection(bootstrap(INSTANCE_A, "16", {
      baseUrl: "not-a-kernel-url",
      release
    }))).toThrow();

    expect(adapter.identity).toBeNull();
    expect(release).toHaveBeenCalledTimes(1);
  });
});

class AdapterHarness {
  readonly invalidations: DesktopKernelDomainInvalidation[] = [];
  readonly states: DesktopKernelEventsStateNotice[] = [];
  readonly sockets: FakeWebSocket[] = [];
  readonly scheduler = new ManualScheduler();
  readonly adapter = createDesktopKernelEventsAdapter({
    onInvalidation: (invalidation) => this.invalidations.push(invalidation),
    onStateChange: (notice) => this.states.push(notice),
    reconnectDelayMs: 25,
    scheduleReconnect: this.scheduler.schedule,
    webSocket: ((url: string) => {
      expect(url).toBe("ws://127.0.0.1:6608/api/v1/events");
      const socket = new FakeWebSocket();
      this.sockets.push(socket);
      return socket;
    }) satisfies WebSocketFactory
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
  readonly #listeners = new Map<
    WebSocketEventType,
    Set<(event: WebSocketEvent) => unknown>
  >();

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

function bootstrap(
  instanceId: string,
  generation: string,
  overrides: Partial<Pick<NativeKernelBootstrap, "baseUrl" | "release">> = {}
): NativeKernelBootstrap {
  return {
    authentication: {
      kind: "native-bearer",
      getCredential: vi.fn(() => SECRET)
    },
    baseUrl: overrides.baseUrl ?? "http://127.0.0.1:6608/",
    generation,
    instanceId,
    release: overrides.release ?? (() => undefined)
  };
}

function readyFrame(connectionId: string, instanceId: string) {
  return {
    type: "ready" as const,
    protocolVersion: 1,
    connectionId,
    instanceId,
    sequence: 0,
    snapshotRequired: true as const
  };
}

function workspaceEvent(connectionId: string, sequence: number, revision: string) {
  return {
    type: "event" as const,
    protocolVersion: 1,
    connectionId,
    sequence,
    resource: { kind: "workspace" as const, id: WORKSPACE_ID },
    revision,
    event: {
      type: "workspace-changed" as const,
      workspace: {
        id: WORKSPACE_ID,
        generation: "workspace-generation-1",
        displayName: "Notes",
        readiness: "ready" as const,
        revision
      }
    }
  };
}

function domainEventFrames(connectionId: string) {
  return [
    workspaceEvent(connectionId, 1, "workspace-revision-1"),
    {
      type: "event" as const,
      protocolVersion: 1,
      connectionId,
      sequence: 2,
      resource: { kind: "document" as const, id: "note.1" },
      revision: "document-revision-1",
      event: {
        type: "document-changed" as const,
        document: {
          id: "note.1",
          kind: "file" as const,
          modifiedAt: "2026-07-30T00:00:00Z",
          name: "note.md",
          parent: "",
          path: "note.md",
          revision: "document-revision-1",
          sizeBytes: 12
        }
      }
    },
    {
      type: "event" as const,
      protocolVersion: 1,
      connectionId,
      sequence: 3,
      resource: { kind: "settings" as const },
      revision: "settings-revision-1",
      event: {
        type: "settings-changed" as const,
        settings: { revision: "settings-revision-1", values: [] }
      }
    },
    {
      type: "event" as const,
      protocolVersion: 1,
      connectionId,
      sequence: 4,
      resource: {
        kind: "app-config" as const,
        workspaceGeneration: "workspace-generation-1",
        workspaceId: WORKSPACE_ID,
      },
      revision: "app-config-revision-1",
      event: {
        type: "app-config-state-changed" as const,
        revision: "app-config-revision-1",
        workspaceGeneration: "workspace-generation-1",
        workspaceId: WORKSPACE_ID
      }
    },
    {
      type: "event" as const,
      protocolVersion: 1,
      connectionId,
      sequence: 5,
      resource: { kind: "sync-config" as const },
      revision: "sync-revision-1",
      event: {
        type: "sync-config-changed" as const,
        config: {
          configured: false,
          enabled: false,
          generateConflictDocument: true,
          intervalSeconds: 300,
          issues: [],
          mode: "automatic" as const,
          provider: "s3" as const,
          readiness: "disabled" as const,
          remoteRoot: "",
          revision: "sync-revision-1",
          s3: {
            accessKeyId: { present: false },
            addressingStyle: "auto" as const,
            bucket: "",
            endpointUrl: { redacted: false, value: null },
            region: "",
            requestTimeoutSeconds: 30,
            secretAccessKey: { present: false },
            tlsVerification: "verify" as const
          },
          webdav: {
            password: { present: false },
            serverUrl: { redacted: false, value: null },
            username: ""
          }
        }
      }
    },
    {
      type: "event" as const,
      protocolVersion: 1,
      connectionId,
      sequence: 6,
      resource: { kind: "sync-status" as const, runId: null },
      revision: "sync-revision-1",
      event: {
        type: "sync-status-changed" as const,
        status: {
          activeRunId: null,
          completionState: "idle" as const,
          configRevision: "sync-revision-1",
          error: null,
          lastAttemptAt: null,
          lastSuccessfulSyncAt: null,
          lastTrigger: null,
          provider: "s3" as const,
          summary: null
        }
      }
    }
  ];
}
