import { describe, expect, it, vi } from "vitest";

import {
  createDesktopKernelStartupOwner,
  type DesktopKernelStartupSnapshot,
  type DesktopKernelStartupSource,
} from "./desktop-kernel-startup";

describe("desktop Kernel startup owner", () => {
  it("subscribes before reading and ignores a stale initial snapshot", async () => {
    const initial = deferred<DesktopKernelStartupSnapshot>();
    const refreshed = deferred<DesktopKernelStartupSnapshot>();
    let handler: (() => unknown) | undefined;
    const source: DesktopKernelStartupSource = {
      listen: vi.fn(async (next) => {
        handler = next;
        return vi.fn();
      }),
      read: vi.fn()
        .mockImplementationOnce(() => initial.promise)
        .mockImplementationOnce(() => refreshed.promise),
    };
    const owner = createDesktopKernelStartupOwner(source);
    const snapshots: DesktopKernelStartupSnapshot[] = [];
    owner.subscribe((snapshot) => snapshots.push(snapshot));

    const startup = owner.start();
    await vi.waitFor(() => expect(handler).toBeTypeOf("function"));
    handler?.();
    refreshed.resolve({ status: "starting" });
    await vi.waitFor(() => expect(snapshots).toEqual([{ status: "starting" }]));
    initial.resolve({ status: "unselected" });
    await startup;

    expect(owner.getSnapshot()).toEqual({ status: "starting" });
    expect(snapshots).toEqual([{ status: "starting" }]);
  });

  it("fails closed to unavailable when the native snapshot cannot be read", async () => {
    const owner = createDesktopKernelStartupOwner({
      listen: vi.fn(async () => vi.fn()),
      read: vi.fn(async () => {
        throw new Error("sensitive native detail");
      }),
    });
    const snapshots: DesktopKernelStartupSnapshot[] = [];
    owner.subscribe((snapshot) => snapshots.push(snapshot));

    await owner.start();

    expect(snapshots).toEqual([{ status: "unavailable" }]);
    expect(JSON.stringify(snapshots)).not.toContain("sensitive native detail");
  });

  it("closes the listener and ignores later native completions", async () => {
    const read = deferred<DesktopKernelStartupSnapshot>();
    const stop = vi.fn();
    const owner = createDesktopKernelStartupOwner({
      listen: vi.fn(async () => stop),
      read: vi.fn(() => read.promise),
    });
    const subscriber = vi.fn();
    owner.subscribe(subscriber);
    const startup = owner.start();
    await Promise.resolve();

    owner.close();
    read.resolve({ status: "ready" });
    await startup;

    expect(stop).toHaveBeenCalledTimes(1);
    expect(subscriber).not.toHaveBeenCalled();
    expect(owner.getSnapshot()).toBeNull();
  });
});

function deferred<T>() {
  let resolve!: (value: T) => undefined;
  const promise = new Promise<T>((complete) => {
    resolve = (value) => {
      complete(value);
      return undefined;
    };
  });
  return { promise, resolve };
}
