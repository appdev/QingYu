import type { KernelDomainPort } from "@markra/app/runtime";

import {
  createDesktopApplicationMountOwner,
  type DesktopApplicationMountOwner
} from "./desktop-application";
import type {
  NativeKernelSessionOwner,
  NativeKernelSessionSnapshot
} from "./runtime/native-kernel-session";

describe("desktop authenticated application mount owner", () => {
  it("keeps the domain application unmounted for every non-ready session state", async () => {
    const session = new SessionHarness();
    const log: string[] = [];
    const mount = createMount(session, log);

    await mount.start();
    session.publish({ domain: null, status: "dormant" });
    session.publish({ domain: null, generation: "1", status: "starting" });
    session.publish({ domain: null, generation: "1", status: "retrying" });
    session.publish({ domain: null, generation: "1", status: "failed" });

    expect(log).toEqual([
      "startup:checking",
      "startup:dormant",
      "startup:starting:1",
      "startup:retrying:1",
      "startup:failed:1"
    ]);
    mount.close();
  });

  it("mounts each authenticated Kernel identity once and remounts one newer generation", async () => {
    const session = new SessionHarness();
    const log: string[] = [];
    const mount = createMount(session, log);
    const first = readySession("7", INSTANCE_A, kernelDomain("first"));
    const second = readySession("8", INSTANCE_B, kernelDomain("second"));

    await mount.start();
    log.length = 0;
    session.publish(first);
    session.publish(first);
    session.publish({ ...first });

    expect(log).toEqual([
      "configure",
      `domain:${INSTANCE_A}:7`
    ]);

    session.publish({ domain: null, generation: "8", status: "retrying" });
    session.publish(second);
    session.publish(second);

    expect(log).toEqual([
      "configure",
      `domain:${INSTANCE_A}:7`,
      `unmount:${INSTANCE_A}:7`,
      "release-runtime",
      "startup:retrying:8",
      "configure",
      `domain:${INSTANCE_B}:8`
    ]);
    mount.close();
  });

  it("unmounts the old identity before a direct ready-generation replacement", async () => {
    const session = new SessionHarness();
    const log: string[] = [];
    const mount = createMount(session, log);

    await mount.start();
    log.length = 0;
    session.publish(readySession("20", INSTANCE_A, kernelDomain("first")));
    session.publish(readySession("21", INSTANCE_B, kernelDomain("second")));

    expect(log).toEqual([
      "configure",
      `domain:${INSTANCE_A}:20`,
      `unmount:${INSTANCE_A}:20`,
      "release-runtime",
      "configure",
      `domain:${INSTANCE_B}:21`
    ]);
    mount.close();
  });

  it("unmounts once and ignores stale callbacks after an idempotent close", async () => {
    const session = new SessionHarness();
    const log: string[] = [];
    const mount = createMount(session, log);
    const firstStart = mount.start();
    const secondStart = mount.start();
    const staleSubscriber = session.subscriberHistory[0];

    expect(firstStart).toBe(secondStart);
    await firstStart;
    log.length = 0;
    session.publish(readySession("9", INSTANCE_A, kernelDomain("first")));
    mount.close();
    mount.close();
    staleSubscriber?.({ domain: null, generation: "10", status: "retrying" });

    expect(log).toEqual([
      "configure",
      `domain:${INSTANCE_A}:9`,
      `unmount:${INSTANCE_A}:9`,
      "release-runtime"
    ]);
    expect(session.start).toHaveBeenCalledTimes(1);
    expect(session.unsubscribe).toHaveBeenCalledTimes(1);
    expect(session.close).toHaveBeenCalledTimes(1);
    await expect(mount.start()).rejects.toThrow(
      "desktop application mount owner closed"
    );
  });

  it.each(["create", "configure", "render"] as const)(
    "fails closed and reports a safe startup failure when %s throws",
    async (failureStage) => {
      const session = new SessionHarness();
      session.publish(readySession("30", INSTANCE_A, kernelDomain("first")));
      const reportFailure = vi.fn();
      const mount = createDesktopApplicationMountOwner({
        configureRuntime: () => {
          if (failureStage === "configure") throw new Error("sensitive configure failure");
        },
        createRuntime: (domain) => {
          if (failureStage === "create") throw new Error("sensitive create failure");
          return { release: () => undefined, runtime: domain };
        },
        owner: session,
        renderDomain: () => {
          if (failureStage === "render") throw new Error("sensitive render failure");
          return undefined;
        },
        renderStartup: () => undefined
      });

      await expect(mount.start(reportFailure)).rejects.toThrow(
        "desktop application mount failed"
      );

      expect(reportFailure).toHaveBeenCalledTimes(1);
      expect(session.close).toHaveBeenCalledTimes(1);
    }
  );

  it("keeps a startup-shell failure generic when closing the session rejects start", async () => {
    const session: NativeKernelSessionOwner = {
      close: vi.fn(() => undefined),
      getSnapshot: () => null,
      start: vi.fn(async () => {
        throw new Error("sensitive native close failure");
      }),
      subscribe: () => () => undefined
    };
    const mount = createDesktopApplicationMountOwner({
      configureRuntime: () => undefined,
      createRuntime: (domain) => ({ release: () => undefined, runtime: domain }),
      owner: session,
      renderDomain: () => undefined,
      renderStartup: () => {
        throw new Error("sensitive startup render failure");
      }
    });

    await expect(mount.start()).rejects.toThrow("desktop application mount failed");
  });
});

const INSTANCE_A = "123e4567-e89b-42d3-a456-426614174000";
const INSTANCE_B = "123e4567-e89b-42d3-a456-426614174001";

class SessionHarness implements NativeKernelSessionOwner {
  private readonly subscribers = new Set<
    (snapshot: NativeKernelSessionSnapshot | null) => unknown
  >();
  private snapshot: NativeKernelSessionSnapshot | null = null;

  readonly start = vi.fn(async () => undefined);
  readonly close = vi.fn(() => undefined);
  readonly unsubscribe = vi.fn(() => undefined);
  readonly subscriberHistory: Array<
    (snapshot: NativeKernelSessionSnapshot | null) => unknown
  > = [];

  subscribe(
    subscriber: (snapshot: NativeKernelSessionSnapshot | null) => unknown
  ) {
    this.subscribers.add(subscriber);
    this.subscriberHistory.push(subscriber);
    if (this.snapshot !== null) subscriber(this.snapshot);
    return () => {
      this.subscribers.delete(subscriber);
      this.unsubscribe();
      return undefined;
    };
  }

  getSnapshot() {
    return this.snapshot;
  }

  publish(snapshot: NativeKernelSessionSnapshot) {
    this.snapshot = snapshot;
    for (const subscriber of [...this.subscribers]) subscriber(snapshot);
  }
}

function createMount(
  session: NativeKernelSessionOwner,
  log: string[]
): DesktopApplicationMountOwner {
  return createDesktopApplicationMountOwner({
    configureRuntime: () => log.push("configure"),
    createRuntime: (_domain: KernelDomainPort) => ({
      release: () => log.push("release-runtime"),
      runtime: { kind: "kernel-runtime" }
    }),
    owner: session,
    renderDomain: ({ mountKey }) => {
      log.push(`domain:${mountKey}`);
      return () => log.push(`unmount:${mountKey}`);
    },
    renderStartup: (snapshot) => log.push([
      "startup",
      snapshot?.status ?? "checking",
      snapshot?.generation
    ].filter(Boolean).join(":"))
  });
}

function readySession(
  generation: string,
  instanceId: string,
  domain: KernelDomainPort
): NativeKernelSessionSnapshot {
  return { domain, generation, instanceId, status: "ready" };
}

function kernelDomain(name: string): KernelDomainPort {
  return Object.freeze({ availability: name }) as unknown as KernelDomainPort;
}
