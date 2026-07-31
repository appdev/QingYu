import { describe, expect, it, vi } from "vitest";

import type { DesktopApplicationMountOwner } from "./desktop-application";
import type {
  DesktopKernelStartupOwner,
  DesktopKernelStartupSnapshot,
  DesktopKernelStartupStatus
} from "./desktop-kernel-startup";
import {
  createDesktopStartupApplicationOwner,
  type DesktopStartupApplicationOwner
} from "./desktop-startup-application";

describe("desktop startup application orchestrator", () => {
  it.each([
    "resolving",
    "unselected",
    "invalid",
    "unsupported-version",
    "unavailable",
    "starting",
    "failed"
  ] as const)("keeps the domain application absent while startup is %s", async (status) => {
    const harness = createHarness();

    await harness.owner.start();
    harness.startup.publish(status);

    expect(harness.createApplicationMountOwner).not.toHaveBeenCalled();
    expect(harness.workspaceStatuses).toEqual([status]);
  });

  it("closes the old domain before a failed fallback and creates one fresh retry", async () => {
    const log: string[] = [];
    const harness = createHarness({ log });

    await harness.owner.start();
    harness.startup.publish("starting");
    harness.startup.publish("ready");
    harness.startup.publish("failed");
    harness.startup.publish("starting");
    harness.startup.publish("ready");

    expect(log).toEqual([
      "workspace:starting",
      "create-domain:1",
      "start-domain:1",
      "close-domain:1",
      "workspace:failed",
      "workspace:starting",
      "create-domain:2",
      "start-domain:2"
    ]);
    expect(harness.createApplicationMountOwner).toHaveBeenCalledTimes(2);
  });

  it("does not let a stale domain start rejection replace a newer ready owner", async () => {
    const firstStart = deferred<undefined>();
    const log: string[] = [];
    const harness = createHarness({
      log,
      mountStartResults: [firstStart.promise, Promise.resolve(undefined)]
    });

    await harness.owner.start();
    harness.startup.publish("starting");
    harness.startup.publish("ready");
    harness.startup.publish("failed");
    harness.startup.publish("starting");
    harness.startup.publish("ready");
    firstStart.reject(new Error("sensitive stale failure"));
    await settlePromises();

    expect(log).toEqual([
      "workspace:starting",
      "create-domain:1",
      "start-domain:1",
      "close-domain:1",
      "workspace:failed",
      "workspace:starting",
      "create-domain:2",
      "start-domain:2"
    ]);
    expect(harness.mounts[1]?.close).not.toHaveBeenCalled();
    expect(JSON.stringify(log)).not.toContain("sensitive stale failure");
  });

  it("reports a current domain start failure to the global reload boundary", async () => {
    const firstStart = deferred<undefined>();
    const log: string[] = [];
    const reportFailure = vi.fn(() => log.push("report-failure"));
    const harness = createHarness({
      log,
      mountStartResults: [firstStart.promise, Promise.resolve(undefined)]
    });

    await harness.owner.start(reportFailure);
    harness.startup.publish("starting");
    harness.startup.publish("ready");
    firstStart.reject(new Error("sensitive current failure"));
    await settlePromises();
    harness.startup.publish("starting");
    harness.startup.publish("ready");

    expect(log).toEqual([
      "workspace:starting",
      "create-domain:1",
      "start-domain:1",
      "close-domain:1",
      "report-failure"
    ]);
    expect(reportFailure).toHaveBeenCalledTimes(1);
    expect(harness.startup.close).toHaveBeenCalledTimes(1);
    expect(harness.createApplicationMountOwner).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(log)).not.toContain("sensitive current failure");
  });

  it("ignores a stale startup-owner rejection after a newer authoritative status", async () => {
    const startupResult = deferred<undefined>();
    const log: string[] = [];
    const harness = createHarness({ log, startupResult: startupResult.promise });

    const starting = harness.owner.start();
    harness.startup.publish("ready");
    startupResult.reject(new Error("sensitive stale startup failure"));
    await starting;

    expect(log).toEqual(["create-domain:1", "start-domain:1"]);
    expect(harness.workspaceStatuses).toEqual([]);
  });

  it("closes the subscription and both owners once and ignores later work", async () => {
    const domainStart = deferred<undefined>();
    const harness = createHarness({ mountStartResults: [domainStart.promise] });
    await harness.owner.start();
    harness.startup.publish("starting");
    harness.startup.publish("ready");

    harness.owner.close();
    harness.owner.close();
    harness.startup.publish("failed");
    domainStart.reject(new Error("sensitive close-race failure"));
    await settlePromises();

    expect(harness.startup.unsubscribe).toHaveBeenCalledTimes(1);
    expect(harness.startup.close).toHaveBeenCalledTimes(1);
    expect(harness.mounts[0]?.close).toHaveBeenCalledTimes(1);
    expect(harness.workspaceStatuses).toEqual(["starting"]);
    await expect(harness.owner.start()).rejects.toThrow(
      "desktop startup application owner closed"
    );
  });

  it("coalesces duplicate workspace and mounting statuses", async () => {
    const harness = createHarness();
    await harness.owner.start();

    harness.startup.publish("unselected");
    harness.startup.publish("unselected");
    harness.startup.publish("starting");
    harness.startup.publish("starting");
    harness.startup.publish("ready");
    harness.startup.publish("ready");

    expect(harness.workspaceStatuses).toEqual(["unselected", "starting"]);
    expect(harness.createApplicationMountOwner).toHaveBeenCalledTimes(1);
    expect(harness.mounts[0]?.start).toHaveBeenCalledTimes(1);
  });
});

class StartupHarness implements DesktopKernelStartupOwner {
  private readonly subscribers = new Set<
    (snapshot: DesktopKernelStartupSnapshot) => unknown
  >();
  private snapshot: DesktopKernelStartupSnapshot | null = null;

  readonly close = vi.fn(() => undefined);
  readonly start: DesktopKernelStartupOwner["start"];
  readonly unsubscribe = vi.fn(() => undefined);

  constructor(startResult: Promise<undefined>) {
    this.start = vi.fn(() => startResult);
  }

  getSnapshot() {
    return this.snapshot;
  }

  publish(status: DesktopKernelStartupStatus) {
    const snapshot = { status } as const;
    this.snapshot = snapshot;
    for (const subscriber of [...this.subscribers]) subscriber(snapshot);
  }

  subscribe(subscriber: (snapshot: DesktopKernelStartupSnapshot) => unknown) {
    this.subscribers.add(subscriber);
    if (this.snapshot !== null) subscriber(this.snapshot);
    return () => {
      this.subscribers.delete(subscriber);
      this.unsubscribe();
      return undefined;
    };
  }
}

interface HarnessOptions {
  readonly log?: string[];
  readonly mountStartResults?: ReadonlyArray<Promise<undefined>>;
  readonly startupResult?: Promise<undefined>;
}

function createHarness(options: HarnessOptions = {}) {
  const log = options.log ?? [];
  const startup = new StartupHarness(
    options.startupResult ?? Promise.resolve(undefined)
  );
  const mounts: Array<{
    readonly close: ReturnType<typeof vi.fn>;
    readonly start: ReturnType<typeof vi.fn>;
  }> = [];
  const createApplicationMountOwner = vi.fn((): DesktopApplicationMountOwner => {
    const identity = mounts.length + 1;
    const close = vi.fn(() => {
      log.push(`close-domain:${identity}`);
      return undefined;
    });
    const start = vi.fn(() => {
      log.push(`start-domain:${identity}`);
      return options.mountStartResults?.[identity - 1]
        ?? Promise.resolve(undefined);
    });
    log.push(`create-domain:${identity}`);
    mounts.push({ close, start });
    return { close, start };
  });
  const workspaceStatuses: DesktopKernelStartupStatus[] = [];
  const owner: DesktopStartupApplicationOwner =
    createDesktopStartupApplicationOwner({
      createApplicationMountOwner,
      renderWorkspace: (snapshot) => {
        workspaceStatuses.push(snapshot.status);
        log.push(`workspace:${snapshot.status}`);
        return undefined;
      },
      startupOwner: startup
    });

  return {
    createApplicationMountOwner,
    log,
    mounts,
    owner,
    startup,
    workspaceStatuses
  };
}

function deferred<T>() {
  let reject!: (cause: unknown) => undefined;
  let resolve!: (value: T) => undefined;
  const promise = new Promise<T>((complete, fail) => {
    reject = (cause) => {
      fail(cause);
      return undefined;
    };
    resolve = (value) => {
      complete(value);
      return undefined;
    };
  });
  return { promise, reject, resolve };
}

async function settlePromises() {
  await Promise.resolve();
  await Promise.resolve();
  return undefined;
}
