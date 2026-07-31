import type { KernelDomainPort } from "@markra/app/runtime";
import { KernelEventError } from "@markra/kernel-client";

import type { NativeKernelBootstrap } from "../kernel-bootstrap";
import type { DesktopKernelDomainAdapter } from "./kernel";
import type {
  DesktopKernelDomainInvalidation,
  DesktopKernelEventsAdapter,
  DesktopKernelEventsAdapterOptions,
  DesktopKernelEventsErrorNotice,
  DesktopKernelEventsStateNotice
} from "./kernel-events";
import {
  createNativeKernelSessionOwner,
  NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT,
  type NativeKernelBootstrapChangedListener,
  type NativeKernelPagehideListener,
  type NativeKernelSessionSnapshot
} from "./native-kernel-session";

const INSTANCE_A = "123e4567-e89b-42d3-a456-426614174000";
const INSTANCE_B = "123e4567-e89b-42d3-a456-426614174001";
const CREDENTIAL_A = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CREDENTIAL_B = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA";

describe("native Kernel session owner", () => {
  it("has no module or construction side effects and survives StrictMode-style resubscription", async () => {
    const listener = new ListenerHarness();
    const domains = new DomainHarness();
    const events = new EventsHarness();
    const invokeCommand = vi.fn(async () => readyBootstrap("1", INSTANCE_A, CREDENTIAL_A));
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand,
      listenBootstrapChanged: listener.listen
    });

    expect(invokeCommand).not.toHaveBeenCalled();
    expect(listener.listen).not.toHaveBeenCalled();
    expect(listener.addPagehideListener).not.toHaveBeenCalled();
    expect(domains.create).not.toHaveBeenCalled();
    expect(events.create).not.toHaveBeenCalled();

    const subscriber = vi.fn();
    const firstCleanup = owner.subscribe(subscriber);
    firstCleanup();
    firstCleanup();
    const secondCleanup = owner.subscribe(subscriber);
    await owner.start();
    secondCleanup();

    expect(listener.listen).toHaveBeenCalledTimes(1);
    expect(listener.addPagehideListener).toHaveBeenCalledTimes(1);
    expect(invokeCommand).toHaveBeenCalledTimes(1);
    expect(domains.create).toHaveBeenCalledTimes(1);
    expect(events.create).toHaveBeenCalledTimes(1);
    expect(events.records[0]?.connections).toHaveLength(1);
    owner.close();
  });

  it("registers the edge listener before the initial truth refresh", async () => {
    const listener = new ListenerHarness();
    const invokeCommand = vi.fn(async () => {
      expect(listener.listen).toHaveBeenCalledTimes(1);
      expect(listener.edge).toBeTypeOf("function");
      return dormantBootstrap();
    });
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      invokeCommand,
      listenBootstrapChanged: listener.listen
    });

    await owner.start();

    expect(owner.getSnapshot()).toEqual({ domain: null, status: "dormant" });
    owner.close();
  });

  it("coalesces concurrent edge signals into one follow-up truth refresh and ignores payload data", async () => {
    const listener = new ListenerHarness();
    const first = deferred<unknown>();
    let calls = 0;
    const invokeCommand = vi.fn(async () => {
      calls += 1;
      if (calls === 1) return first.promise;
      return lifecycleBootstrap("retrying", "2");
    });
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      invokeCommand,
      listenBootstrapChanged: listener.listen
    });
    const started = owner.start();
    await vi.waitFor(() => expect(invokeCommand).toHaveBeenCalledTimes(1));
    const payload = Object.defineProperties({}, {
      credential: {
        get: () => {
          throw new Error("event credential was read");
        }
      },
      port: {
        get: () => {
          throw new Error("event port was read");
        }
      }
    });

    const signals = [
      listener.signal(payload),
      listener.signal(payload),
      listener.signal(payload)
    ];
    first.resolve(lifecycleBootstrap("starting", "1"));
    await Promise.all([started, ...signals]);

    expect(invokeCommand).toHaveBeenCalledTimes(2);
    expect(owner.getSnapshot()).toEqual({
      domain: null,
      generation: "2",
      status: "retrying"
    });
    owner.close();
  });

  it("retains an edge that arrives during a rejected refresh and automatically refreshes truth again", async () => {
    const listener = new ListenerHarness();
    const rejectedRefresh = deferred<unknown>();
    const responses = [
      lifecycleBootstrap("starting", "1"),
      rejectedRefresh.promise,
      lifecycleBootstrap("retrying", "2")
    ];
    const invokeCommand = vi.fn(async () => responses.shift());
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      invokeCommand,
      listenBootstrapChanged: listener.listen
    });
    await owner.start();

    const failingRefresh = listener.signal({ edge: "first" });
    await vi.waitFor(() => expect(invokeCommand).toHaveBeenCalledTimes(2));
    const dirtyRefresh = listener.signal({ edge: "later" });
    rejectedRefresh.reject(new Error("transient refresh failure"));
    await Promise.all([failingRefresh, dirtyRefresh]);

    expect(invokeCommand).toHaveBeenCalledTimes(3);
    expect(owner.getSnapshot()).toEqual({
      domain: null,
      generation: "2",
      status: "retrying"
    });
    owner.close();
  });

  it.each([
    ["dormant", dormantBootstrap(), { domain: null, status: "dormant" }],
    ["starting", lifecycleBootstrap("starting", "3"), {
      domain: null,
      generation: "3",
      status: "starting"
    }],
    ["retrying", lifecycleBootstrap("retrying", "4"), {
      domain: null,
      generation: "4",
      status: "retrying"
    }],
    ["failed", lifecycleBootstrap("failed", "5"), {
      domain: null,
      generation: "5",
      status: "failed"
    }]
  ] as const)(
    "does not create domain or event adapters for %s",
    async (_name, response, expected) => {
      const listener = new ListenerHarness();
      const domains = new DomainHarness();
      const events = new EventsHarness();
      const owner = createNativeKernelSessionOwner({
        addPagehideListener: listener.addPagehideListener,
        createDomainAdapter: domains.create,
        createEventsAdapter: events.create,
        invokeCommand: async () => response,
        listenBootstrapChanged: listener.listen
      });

      await owner.start();

      expect(owner.getSnapshot()).toEqual(expected);
      expect(domains.create).not.toHaveBeenCalled();
      expect(events.create).not.toHaveBeenCalled();
      owner.close();
    }
  );

  it("hands ready domain and events adapters independent credential leases", async () => {
    const listener = new ListenerHarness();
    const domains = new DomainHarness();
    const events = new EventsHarness();
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => readyBootstrap("6", INSTANCE_A, CREDENTIAL_A),
      listenBootstrapChanged: listener.listen
    });

    await owner.start();

    const domainLease = domains.leases[0];
    const eventsLease = events.records[0]?.connections[0];
    expect(domainLease).toBeDefined();
    expect(eventsLease).toBeDefined();
    expect(domainLease).not.toBe(eventsLease);
    expect(domainLease?.authentication.getCredential()).toBe(CREDENTIAL_A);
    expect(eventsLease?.authentication.getCredential()).toBe(CREDENTIAL_A);
    expect(owner.getSnapshot()).toMatchObject({
      domain: domains.adapters[0]?.port,
      generation: "6",
      instanceId: INSTANCE_A,
      status: "ready"
    });
    owner.close();
  });

  it("publishes synchronous event adoption notices in order after the ready session commits", async () => {
    const listener = new ListenerHarness();
    const domains = new DomainHarness();
    const deliveries: string[] = [];
    let owner: ReturnType<typeof createNativeKernelSessionOwner>;
    owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: (options) => {
        let adopted: NativeKernelBootstrap | undefined;
        const close = vi.fn(() => {
          adopted?.release();
          adopted = undefined;
          return undefined;
        });
        return {
          get identity() {
            return adopted === undefined ? null : {
              generation: adopted.generation,
              instanceId: adopted.instanceId
            };
          },
          close,
          replaceConnection: (bootstrap) => {
            expect(bootstrap).not.toBeNull();
            adopted = bootstrap ?? undefined;
            const identity = {
              generation: bootstrap!.generation,
              instanceId: bootstrap!.instanceId
            };
            options.onStateChange?.({ ...identity, state: "connecting" });
            options.onError?.({
              ...identity,
              error: new KernelEventError("connection")
            });
            options.onStateChange?.({ ...identity, state: "reconnecting" });
            return undefined;
          }
        };
      },
      invokeCommand: async () => readyBootstrap("6", INSTANCE_A, CREDENTIAL_A),
      listenBootstrapChanged: listener.listen,
      onEventsError: (notice) => {
        deliveries.push(`${owner.getSnapshot()?.status}:${notice.error.kind}`);
      },
      onEventsStateChange: (notice) => {
        deliveries.push(`${owner.getSnapshot()?.status}:${notice.state}`);
      }
    });

    await owner.start();

    expect(deliveries).toEqual([
      "ready:connecting",
      "ready:connection",
      "ready:reconnecting"
    ]);
    owner.close();
  });

  it.each(["before-callback", "after-callback"] as const)(
    "fails closed and retires both leases when event connection adoption throws %s",
    async (failurePoint) => {
      const listener = new ListenerHarness();
      const domains = new DomainHarness();
      const invalidations: DesktopKernelDomainInvalidation[] = [];
      const eventErrors: DesktopKernelEventsErrorNotice[] = [];
      const eventStates: DesktopKernelEventsStateNotice[] = [];
      let capturedEventsLease: NativeKernelBootstrap | undefined;
      const closeEvents = vi.fn(() => {
        if (failurePoint === "after-callback") capturedEventsLease?.release();
        return undefined;
      });
      const owner = createNativeKernelSessionOwner({
        addPagehideListener: listener.addPagehideListener,
        createDomainAdapter: domains.create,
        createEventsAdapter: (options) => ({
          identity: null,
          close: closeEvents,
          replaceConnection: (bootstrap) => {
            capturedEventsLease = bootstrap ?? undefined;
            if (failurePoint === "after-callback") {
              const identity = { generation: "6", instanceId: INSTANCE_A };
              options.onStateChange?.({ ...identity, state: "connecting" });
              options.onError?.({
                ...identity,
                error: new KernelEventError("connection")
              });
              options.onInvalidation(snapshotInvalidation(INSTANCE_A, "6"));
              options.onStateChange?.({ ...identity, state: "reconnecting" });
            }
            throw new Error("event connection adoption failed");
          }
        }),
        invokeCommand: async () => readyBootstrap("6", INSTANCE_A, CREDENTIAL_A),
        listenBootstrapChanged: listener.listen,
        onEventsError: (notice) => eventErrors.push(notice),
        onEventsStateChange: (notice) => eventStates.push(notice),
        onInvalidation: (invalidation) => invalidations.push(invalidation)
      });

      await expect(owner.start()).rejects.toThrow("event connection adoption failed");
      try {
        expect(owner.getSnapshot()).toBeNull();
        expect(() => capturedEventsLease?.authentication.getCredential()).toThrow(
          "native Kernel credential unavailable"
        );
        expect(eventErrors).toEqual([]);
        expect(eventStates).toEqual([]);
        expect(invalidations).toEqual([]);
        expect(closeEvents).toHaveBeenCalledTimes(1);
        expect(domains.adapters[0]?.release).toHaveBeenCalledTimes(1);
      } finally {
        owner.close();
      }
    }
  );

  it("retires the old domain and event socket before publishing changed and non-ready states", async () => {
    const log: string[] = [];
    const listener = new ListenerHarness();
    const domains = new DomainHarness(log);
    const events = new EventsHarness(log);
    const responses = [
      readyBootstrap("7", INSTANCE_A, CREDENTIAL_A),
      readyBootstrap("8", INSTANCE_B, CREDENTIAL_B),
      lifecycleBootstrap("retrying", "9")
    ];
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => responses.shift(),
      listenBootstrapChanged: listener.listen
    });
    owner.subscribe((snapshot) => {
      log.push(
        `publish-${snapshot?.status ?? "unavailable"}-${snapshot?.generation ?? "none"}`
      );
    });
    await owner.start();
    log.length = 0;

    await listener.signal({ untrusted: true });

    expect(log).toEqual([
      "domain-1-close",
      "events-1-close",
      "domain-2-open",
      "events-2-open",
      "publish-ready-8"
    ]);
    log.length = 0;

    await listener.signal({ untrusted: true });

    expect(log).toEqual([
      "domain-2-close",
      "events-2-close",
      "publish-retrying-9"
    ]);
    owner.close();
  });

  it("ignores callbacks from a retired generation after a replacement is active", async () => {
    const listener = new ListenerHarness();
    const domains = new DomainHarness();
    const events = new EventsHarness();
    const invalidations: DesktopKernelDomainInvalidation[] = [];
    const responses = [
      readyBootstrap("10", INSTANCE_A, CREDENTIAL_A),
      readyBootstrap("11", INSTANCE_B, CREDENTIAL_B)
    ];
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => responses.shift(),
      listenBootstrapChanged: listener.listen,
      onInvalidation: (invalidation) => invalidations.push(invalidation)
    });
    await owner.start();
    await listener.signal({});
    const stale = snapshotInvalidation(INSTANCE_A, "10");
    const current = snapshotInvalidation(INSTANCE_B, "11");

    events.records[0]?.options.onInvalidation(stale);
    events.records[1]?.options.onInvalidation(current);

    expect(invalidations).toEqual([current]);
    owner.close();
  });

  it("ignores queued callbacks from a retired adoption when the same identity is adopted again", async () => {
    const listener = new ListenerHarness();
    const domains = new DomainHarness();
    const events = new EventsHarness();
    const invalidations: DesktopKernelDomainInvalidation[] = [];
    const responses = [
      readyBootstrap("10", INSTANCE_A, CREDENTIAL_A),
      new Error("native Kernel bootstrap refresh failed"),
      readyBootstrap("10", INSTANCE_A, CREDENTIAL_A)
    ];
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => {
        const response = responses.shift();
        if (response instanceof Error) throw response;
        return response;
      },
      listenBootstrapChanged: listener.listen,
      onInvalidation: (invalidation) => invalidations.push(invalidation)
    });
    await owner.start();
    await listener.signal({ failed: true });
    await listener.signal({ recovered: true });
    const queuedStale = snapshotInvalidation(INSTANCE_A, "10");
    const current = snapshotInvalidation(INSTANCE_A, "10");

    events.records[0]?.options.onInvalidation(queuedStale);
    events.records[1]?.options.onInvalidation(current);

    expect(events.records).toHaveLength(2);
    expect(invalidations).toEqual([current]);
    owner.close();
  });

  it("fails closed on a generation regression and recovers only from a newer truth", async () => {
    const log: string[] = [];
    const listener = new ListenerHarness();
    const domains = new DomainHarness(log);
    const events = new EventsHarness(log);
    const errors: Error[] = [];
    const snapshots: Array<NativeKernelSessionSnapshot | null> = [];
    const responses = [
      readyBootstrap("12", INSTANCE_A, CREDENTIAL_A),
      readyBootstrap("11", INSTANCE_B, CREDENTIAL_B),
      readyBootstrap("13", INSTANCE_B, CREDENTIAL_B)
    ];
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => responses.shift(),
      listenBootstrapChanged: listener.listen,
      onError: (error) => errors.push(error)
    });
    owner.subscribe((snapshot) => snapshots.push(snapshot));
    await owner.start();
    log.length = 0;

    await listener.signal({});

    expect(log).toEqual(["domain-1-close", "events-1-close"]);
    expect(owner.getSnapshot()).toBeNull();
    expect(snapshots.at(-1)).toBeNull();
    expect(errors.map(String)).toEqual([
      "Error: native Kernel bootstrap generation regressed"
    ]);
    expect(domains.create).toHaveBeenCalledTimes(1);
    expect(events.create).toHaveBeenCalledTimes(1);

    await listener.signal({});

    expect(owner.getSnapshot()).toMatchObject({
      generation: "13",
      status: "ready"
    });
    expect(domains.create).toHaveBeenCalledTimes(2);
    expect(events.create).toHaveBeenCalledTimes(2);
    owner.close();
  });

  it("retires a ready session before pagehide publishes unavailable exactly once", async () => {
    const log: string[] = [];
    const listener = new ListenerHarness();
    const domains = new DomainHarness(log);
    const events = new EventsHarness(log);
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => readyBootstrap("14", INSTANCE_A, CREDENTIAL_A),
      listenBootstrapChanged: listener.listen
    });
    owner.subscribe((snapshot) => {
      log.push(`publish-${snapshot?.status ?? "unavailable"}`);
    });
    await owner.start();
    log.length = 0;

    listener.pagehide?.();
    listener.pagehide?.();
    owner.close();

    expect(log).toEqual([
      "domain-1-close",
      "events-1-close",
      "publish-unavailable"
    ]);
  });

  it("closes pagehide, listener, domain, socket, and leases exactly once", async () => {
    const listener = new ListenerHarness();
    const domains = new DomainHarness();
    const events = new EventsHarness();
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => readyBootstrap("14", INSTANCE_A, CREDENTIAL_A),
      listenBootstrapChanged: listener.listen
    });
    await owner.start();

    listener.pagehide?.();
    listener.pagehide?.();
    owner.close();
    owner.close();

    expect(listener.unlisten).toHaveBeenCalledTimes(1);
    expect(listener.removePagehide).toHaveBeenCalledTimes(1);
    expect(domains.adapters[0]?.release).toHaveBeenCalledTimes(1);
    expect(events.records[0]?.close).toHaveBeenCalledTimes(1);
    expect(() => domains.leases[0]?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    expect(() => (
      events.records[0]?.connections[0]?.authentication.getCredential()
    )).toThrow("native Kernel credential unavailable");
  });

  it("stops a ready publication when an earlier subscriber closes the owner", async () => {
    const listener = new ListenerHarness();
    const domains = new DomainHarness();
    const events = new EventsHarness();
    const laterSnapshots: Array<NativeKernelSessionSnapshot | null> = [];
    const owner = createNativeKernelSessionOwner({
      addPagehideListener: listener.addPagehideListener,
      createDomainAdapter: domains.create,
      createEventsAdapter: events.create,
      invokeCommand: async () => readyBootstrap("15", INSTANCE_A, CREDENTIAL_A),
      listenBootstrapChanged: listener.listen
    });
    owner.subscribe((next) => {
      if (next?.status === "ready") owner.close();
    });
    owner.subscribe((next) => laterSnapshots.push(next));

    await owner.start();

    expect(owner.getSnapshot()).toBeNull();
    expect(laterSnapshots).toEqual([]);
    expect(domains.adapters[0]?.release).toHaveBeenCalledTimes(1);
    expect(events.records[0]?.close).toHaveBeenCalledTimes(1);
  });
});

class ListenerHarness {
  edge: ((event: unknown) => unknown) | undefined;
  pagehide: (() => unknown) | undefined;
  readonly unlisten = vi.fn(() => undefined);
  readonly removePagehide = vi.fn(() => undefined);
  readonly listen: NativeKernelBootstrapChangedListener = vi.fn(async (eventName, handler) => {
    expect(eventName).toBe(NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT);
    this.edge = handler;
    return this.unlisten;
  });
  readonly addPagehideListener: NativeKernelPagehideListener = vi.fn((handler) => {
    this.pagehide = handler;
    return this.removePagehide;
  });

  async signal(payload: unknown): Promise<unknown> {
    if (this.edge === undefined) throw new Error("edge listener is unavailable");
    return this.edge(payload);
  }
}

class DomainHarness {
  readonly adapters: DesktopKernelDomainAdapter[] = [];
  readonly leases: NativeKernelBootstrap[] = [];
  readonly create = vi.fn(async (lease: NativeKernelBootstrap) => {
    this.leases.push(lease);
    const number = this.adapters.length + 1;
    this.log?.push(`domain-${number}-open`);
    let released = false;
    const release = vi.fn(() => {
      if (released) return undefined;
      released = true;
      this.log?.push(`domain-${number}-close`);
      lease.release();
      return undefined;
    });
    const adapter = {
      port: Object.freeze({ availability: "available" }) as KernelDomainPort,
      release
    } satisfies DesktopKernelDomainAdapter;
    this.adapters.push(adapter);
    return adapter;
  });

  constructor(private readonly log?: string[]) {}
}

interface EventsRecord {
  readonly adapter: DesktopKernelEventsAdapter;
  readonly close: ReturnType<typeof vi.fn>;
  readonly connections: NativeKernelBootstrap[];
  readonly options: DesktopKernelEventsAdapterOptions;
}

class EventsHarness {
  readonly records: EventsRecord[] = [];
  readonly create = vi.fn((options: DesktopKernelEventsAdapterOptions) => {
    const number = this.records.length + 1;
    const connections: NativeKernelBootstrap[] = [];
    let active: NativeKernelBootstrap | undefined;
    let closed = false;
    const close = vi.fn(() => {
      if (closed) return undefined;
      closed = true;
      this.log?.push(`events-${number}-close`);
      active?.release();
      active = undefined;
      return undefined;
    });
    const adapter: DesktopKernelEventsAdapter = {
      get identity() {
        return active === undefined ? null : {
          generation: active.generation,
          instanceId: active.instanceId
        };
      },
      close,
      replaceConnection: (bootstrap) => {
        if (bootstrap === null) return close();
        active = bootstrap;
        connections.push(bootstrap);
        this.log?.push(`events-${number}-open`);
        return undefined;
      }
    };
    const record = { adapter, close, connections, options };
    this.records.push(record);
    return adapter;
  });

  constructor(private readonly log?: string[]) {}
}

function readyBootstrap(
  generation: string,
  instanceId: string,
  credential: string
) {
  return {
    bootstrapVersion: 1,
    credential,
    generation,
    instanceId,
    port: generation === "1" ? 49_152 : 49_153,
    status: "ready"
  };
}

function dormantBootstrap() {
  return { bootstrapVersion: 1, status: "dormant" };
}

function lifecycleBootstrap(
  status: "starting" | "retrying" | "failed",
  generation: string
) {
  return { bootstrapVersion: 1, generation, status };
}

function snapshotInvalidation(
  instanceId: string,
  generation: string
): DesktopKernelDomainInvalidation {
  return {
    generation,
    instanceId,
    kind: "snapshot-required",
    reason: "sequence-gap",
    scopes: ["documents"]
  };
}

function deferred<T>() {
  let resolvePromise: ((value: T) => undefined) | undefined;
  let rejectPromise: ((cause?: unknown) => undefined) | undefined;
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = (value) => {
      resolve(value);
      return undefined;
    };
    rejectPromise = (cause) => {
      reject(cause);
      return undefined;
    };
  });
  return {
    promise,
    reject: (cause?: unknown) => rejectPromise?.(cause),
    resolve: (value: T) => resolvePromise?.(value)
  };
}
