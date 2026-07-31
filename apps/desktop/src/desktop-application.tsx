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

export interface DesktopApplicationMountOwner {
  start(): Promise<undefined>;
  close(): undefined;
}

export interface DesktopApplicationMountOptions<Runtime> {
  readonly configureRuntime: (runtime: Runtime) => unknown;
  readonly createRuntime: (domain: KernelDomainPort) => Runtime;
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
    return undefined;
  };

  const handleSession = (session: NativeKernelSessionSnapshot | null) => {
    if (closed) return undefined;
    if (session?.status === "ready") {
      const mountKey = `${session.instanceId}:${session.generation}`;
      if (mountedIdentity === mountKey) return undefined;
      unmountActiveDomain();
      const runtime = createRuntime(session.domain);
      configureRuntime(runtime);
      unmountDomain = renderDomain({ mountKey, runtime, session });
      mountedIdentity = mountKey;
      return undefined;
    }
    unmountActiveDomain();
    renderStartup(session);
    return undefined;
  };

  const start = () => {
    if (closed) {
      return Promise.reject(new Error("desktop application mount owner closed"));
    }
    if (startPromise !== undefined) return startPromise;

    unsubscribe = owner.subscribe(handleSession);
    if (owner.getSnapshot() === null) handleSession(null);
    startPromise = owner.start();
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
