import type { KernelDomainPort } from "@markra/app/runtime";

import type {
  NativeKernelSessionOwner,
  NativeKernelSessionSnapshot
} from "./runtime/native-kernel-session";

export type DesktopReadyKernelSession = Extract<
  NativeKernelSessionSnapshot,
  { readonly status: "ready" }
>;

export type DesktopStartupKernelSession = Exclude<
  NativeKernelSessionSnapshot,
  { readonly status: "ready" }
> | null;

export interface DesktopApplicationDomainMount<Runtime> {
  readonly mountKey: string;
  readonly runtime: Runtime;
  readonly session: DesktopReadyKernelSession;
}

export interface DesktopApplicationRuntimeOwner<Runtime> {
  readonly runtime: Runtime;
  readonly release: () => unknown;
}

export interface DesktopApplicationMountOwner {
  start(onFailure?: () => unknown): Promise<undefined>;
  close(): undefined;
}

export interface DesktopApplicationMountOptions<Runtime> {
  readonly configureRuntime: (runtime: Runtime) => unknown;
  readonly createRuntime: (
    domain: KernelDomainPort
  ) => DesktopApplicationRuntimeOwner<Runtime>;
  readonly owner: NativeKernelSessionOwner;
  readonly renderDomain: (
    mount: DesktopApplicationDomainMount<Runtime>
  ) => (() => unknown) | undefined;
  readonly renderStartup: (session: DesktopStartupKernelSession) => unknown;
}

export function createDesktopApplicationMountOwner<Runtime>(
  {
    configureRuntime,
    createRuntime,
    owner,
    renderDomain,
    renderStartup
  }: DesktopApplicationMountOptions<Runtime>
): DesktopApplicationMountOwner {
  let closed = false;
  let mountedIdentity: string | undefined;
  let mountFailure: Error | undefined;
  let reportFailure: (() => unknown) | undefined;
  let releaseRuntime: (() => unknown) | undefined;
  let unmountDomain: (() => unknown) | undefined;
  let startPromise: Promise<undefined> | undefined;
  let unsubscribe: (() => unknown) | undefined;

  const unmountActiveDomain = () => {
    const unmount = unmountDomain;
    unmountDomain = undefined;
    mountedIdentity = undefined;
    try {
      unmount?.();
    } catch {
      // Renderer cleanup must not interrupt credential and session retirement.
    }
    const release = releaseRuntime;
    releaseRuntime = undefined;
    try {
      release?.();
    } catch {
      // Runtime-local resources retire before the native session releases credentials.
    }
    return undefined;
  };

  const handleSession = (session: NativeKernelSessionSnapshot | null) => {
    if (closed) return undefined;
    try {
      if (session?.status === "ready") {
        const mountKey = `${session.instanceId}:${session.generation}`;
        if (mountedIdentity === mountKey) return undefined;
        unmountActiveDomain();
        const runtimeOwner = createRuntime(session.domain);
        releaseRuntime = once(runtimeOwner.release);
        configureRuntime(runtimeOwner.runtime);
        unmountDomain = renderDomain({
          mountKey,
          runtime: runtimeOwner.runtime,
          session
        });
        mountedIdentity = mountKey;
        return undefined;
      }
      unmountActiveDomain();
      renderStartup(session);
      return undefined;
    } catch {
      mountFailure = new Error("desktop application mount failed");
      close();
      try {
        reportFailure?.();
      } catch {
        // Failure UI errors cannot interrupt native session retirement.
      }
      return undefined;
    }
  };

  const start = (onFailure?: () => unknown) => {
    if (closed) {
      return Promise.reject(mountFailure ?? new Error("desktop application mount owner closed"));
    }
    reportFailure ??= onFailure;
    if (startPromise !== undefined) return startPromise;

    const stopSubscription = owner.subscribe(handleSession);
    if (closed) {
      stopSubscription();
      return Promise.reject(
        mountFailure ?? new Error("desktop application mount owner closed")
      );
    }
    unsubscribe = stopSubscription;
    if (owner.getSnapshot() === null) handleSession(null);
    startPromise = owner.start().then(
      () => {
        if (mountFailure !== undefined) throw mountFailure;
        return undefined;
      },
      (cause: unknown) => {
        if (mountFailure !== undefined) throw mountFailure;
        throw cause;
      }
    );
    return startPromise;
  };

  const close = () => {
    if (closed) return undefined;
    closed = true;
    unsubscribe?.();
    unsubscribe = undefined;
    unmountActiveDomain();
    owner.close();
    return undefined;
  };

  return Object.freeze({
    start,
    close
  });
}

function once(operation: () => unknown): () => undefined {
  let active = true;
  return () => {
    if (!active) return undefined;
    active = false;
    operation();
    return undefined;
  };
}
