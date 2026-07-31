import { listenNativeEvent } from "./runtime/tauri/events";
import { invokeNative } from "./runtime/tauri/invoke";

export const DESKTOP_KERNEL_STARTUP_CHANGED_EVENT =
  "qingyu://desktop-kernel-startup-changed";

export type DesktopKernelStartupStatus =
  | "resolving"
  | "unselected"
  | "invalid"
  | "unsupported-version"
  | "unavailable"
  | "starting"
  | "ready"
  | "failed";

export interface DesktopKernelStartupSnapshot {
  readonly status: DesktopKernelStartupStatus;
}

export interface DesktopKernelStartupSource {
  readonly listen: (handler: () => unknown) => Promise<() => unknown>;
  readonly read: () => Promise<DesktopKernelStartupSnapshot>;
}

export interface DesktopKernelStartupOwner {
  readonly close: () => undefined;
  readonly getSnapshot: () => DesktopKernelStartupSnapshot | null;
  readonly start: () => Promise<undefined>;
  readonly subscribe: (
    subscriber: (snapshot: DesktopKernelStartupSnapshot) => unknown,
  ) => () => undefined;
}

export function createDesktopKernelStartupOwner(
  source: DesktopKernelStartupSource = nativeDesktopKernelStartupSource,
): DesktopKernelStartupOwner {
  const subscribers = new Set<
    (snapshot: DesktopKernelStartupSnapshot) => unknown
  >();
  let closed = false;
  let requestSequence = 0;
  let snapshot: DesktopKernelStartupSnapshot | null = null;
  let startPromise: Promise<undefined> | undefined;
  let stopListener: (() => unknown) | undefined;

  const publish = (next: DesktopKernelStartupSnapshot) => {
    if (closed) return undefined;
    snapshot = next;
    for (const subscriber of [...subscribers]) {
      try {
        subscriber(next);
      } catch {
        // Renderer consumers cannot break native startup-state ownership.
      }
    }
    return undefined;
  };

  const refresh = async () => {
    const request = ++requestSequence;
    let next: DesktopKernelStartupSnapshot;
    try {
      next = await source.read();
    } catch {
      next = { status: "unavailable" };
    }
    if (!closed && request === requestSequence) publish(next);
    return undefined;
  };

  const close = () => {
    if (closed) return undefined;
    closed = true;
    requestSequence += 1;
    snapshot = null;
    subscribers.clear();
    const stop = stopListener;
    stopListener = undefined;
    try {
      stop?.();
    } catch {
      // Native listener cleanup is best effort during page teardown.
    }
    return undefined;
  };

  const start = () => {
    if (closed) return Promise.reject(new Error("desktop Kernel startup owner closed"));
    if (startPromise !== undefined) return startPromise;
    startPromise = (async () => {
      let stop: () => unknown;
      try {
        stop = await source.listen(() => refresh());
      } catch {
        publish({ status: "unavailable" });
        return undefined;
      }
      if (closed) {
        try {
          stop();
        } catch {
          // Native listener cleanup is best effort during a close race.
        }
        return undefined;
      }
      stopListener = stop;
      await refresh();
      return undefined;
    })();
    return startPromise;
  };

  return Object.freeze({
    close,
    getSnapshot: () => snapshot,
    start,
    subscribe: (subscriber: (snapshot: DesktopKernelStartupSnapshot) => unknown) => {
      if (closed) throw new Error("desktop Kernel startup owner closed");
      subscribers.add(subscriber);
      if (snapshot !== null) publishToSubscriber(subscriber, snapshot);
      return () => {
        subscribers.delete(subscriber);
        return undefined;
      };
    },
  });
}

export function initializeDesktopKernelWorkspace(workspacePath: string) {
  return invokeNative<unknown>("initialize_desktop_kernel_workspace", {
    path: workspacePath,
  });
}

export function switchDesktopKernelWorkspace(workspacePath: string) {
  return invokeNative<unknown>("switch_desktop_kernel_workspace", {
    path: workspacePath,
  });
}

export function retryDesktopKernelWorkspace() {
  return invokeNative<unknown>("retry_desktop_kernel_workspace");
}

const nativeDesktopKernelStartupSource: DesktopKernelStartupSource = {
  listen: (handler) => listenNativeEvent(
    DESKTOP_KERNEL_STARTUP_CHANGED_EVENT,
    () => handler(),
  ),
  read: async () => normalizeDesktopKernelStartupSnapshot(
    await invokeNative<unknown>("read_desktop_kernel_startup_state"),
  ),
};

function normalizeDesktopKernelStartupSnapshot(
  value: unknown,
): DesktopKernelStartupSnapshot {
  if (
    typeof value !== "object"
    || value === null
    || !("status" in value)
    || !isDesktopKernelStartupStatus(value.status)
  ) {
    throw new Error("desktop Kernel startup state is unavailable");
  }
  return Object.freeze({ status: value.status });
}

function isDesktopKernelStartupStatus(
  value: unknown,
): value is DesktopKernelStartupStatus {
  return value === "resolving"
    || value === "unselected"
    || value === "invalid"
    || value === "unsupported-version"
    || value === "unavailable"
    || value === "starting"
    || value === "ready"
    || value === "failed";
}

function publishToSubscriber(
  subscriber: (snapshot: DesktopKernelStartupSnapshot) => unknown,
  snapshot: DesktopKernelStartupSnapshot,
) {
  try {
    subscriber(snapshot);
  } catch {
    // A newly subscribed renderer cannot break native startup-state ownership.
  }
  return undefined;
}
