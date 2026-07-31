import type { DesktopApplicationMountOwner } from "./desktop-application";
import type {
  DesktopKernelStartupOwner,
  DesktopKernelStartupSnapshot
} from "./desktop-kernel-startup";

export interface DesktopStartupApplicationOptions {
  readonly createApplicationMountOwner: () => DesktopApplicationMountOwner;
  readonly renderWorkspace: (
    snapshot: DesktopKernelStartupSnapshot
  ) => unknown;
  readonly startupOwner: DesktopKernelStartupOwner;
}

export interface DesktopStartupApplicationOwner {
  readonly close: () => undefined;
  readonly start: () => Promise<undefined>;
}

export function createDesktopStartupApplicationOwner(
  {
    createApplicationMountOwner,
    renderWorkspace,
    startupOwner
  }: DesktopStartupApplicationOptions
): DesktopStartupApplicationOwner {
  let closed = false;
  let domainOwner: DesktopApplicationMountOwner | undefined;
  let domainRevision = 0;
  let lastWorkspaceStatus: DesktopKernelStartupSnapshot["status"] | undefined;
  let latestSnapshot: DesktopKernelStartupSnapshot | null = null;
  let snapshotRevision = 0;
  let startPromise: Promise<undefined> | undefined;
  let unsubscribe: (() => unknown) | undefined;

  const closeDomainOwner = () => {
    domainRevision += 1;
    const owner = domainOwner;
    domainOwner = undefined;
    try {
      owner?.close();
    } catch {
      // Domain cleanup is best effort while the authoritative shell takes over.
    }
    return undefined;
  };

  const renderWorkspaceSnapshot = (
    snapshot: DesktopKernelStartupSnapshot
  ) => {
    if (lastWorkspaceStatus === snapshot.status) return undefined;
    lastWorkspaceStatus = snapshot.status;
    try {
      renderWorkspace(snapshot);
    } catch {
      // Workspace rendering cannot reactivate or retain a domain owner.
    }
    return undefined;
  };

  const failDomainStart = (
    owner: DesktopApplicationMountOwner | undefined,
    revision: number
  ) => {
    if (
      closed
      || domainRevision !== revision
      || (owner !== undefined && domainOwner !== owner)
    ) {
      return undefined;
    }
    domainOwner = undefined;
    domainRevision += 1;
    try {
      owner?.close();
    } catch {
      // A failed domain owner cannot prevent the workspace shell fallback.
    }
    renderWorkspaceSnapshot({ status: "failed" });
    return undefined;
  };

  const createDomainOwner = () => {
    const revision = ++domainRevision;
    let owner: DesktopApplicationMountOwner;
    try {
      owner = createApplicationMountOwner();
    } catch {
      failDomainStart(undefined, revision);
      return undefined;
    }
    if (closed || domainRevision !== revision) {
      try {
        owner.close();
      } catch {
        // A superseded owner is already detached from the renderer lifecycle.
      }
      return undefined;
    }
    domainOwner = owner;

    let starting: Promise<undefined>;
    try {
      starting = owner.start();
    } catch {
      failDomainStart(owner, revision);
      return undefined;
    }
    starting.then(
      () => undefined,
      () => failDomainStart(owner, revision)
    );
    return undefined;
  };

  const handleSnapshot = (snapshot: DesktopKernelStartupSnapshot) => {
    if (closed) return undefined;
    latestSnapshot = snapshot;
    snapshotRevision += 1;
    if (snapshot.status === "starting" || snapshot.status === "ready") {
      lastWorkspaceStatus = undefined;
      if (domainOwner === undefined) createDomainOwner();
      return undefined;
    }
    closeDomainOwner();
    renderWorkspaceSnapshot(snapshot);
    return undefined;
  };

  const handleStartupFailure = (revision: number) => {
    if (
      closed
      || latestSnapshot !== null
      || snapshotRevision !== revision
    ) {
      return undefined;
    }
    handleSnapshot({ status: "unavailable" });
    return undefined;
  };

  const start = () => {
    if (closed) {
      return Promise.reject(new Error("desktop startup application owner closed"));
    }
    if (startPromise !== undefined) return startPromise;

    try {
      unsubscribe = startupOwner.subscribe(handleSnapshot);
    } catch {
      handleStartupFailure(snapshotRevision);
      startPromise = Promise.resolve(undefined);
      return startPromise;
    }
    const existingSnapshot = startupOwner.getSnapshot();
    if (existingSnapshot !== null) handleSnapshot(existingSnapshot);
    const revision = snapshotRevision;

    let starting: Promise<undefined>;
    try {
      starting = startupOwner.start();
    } catch {
      handleStartupFailure(revision);
      startPromise = Promise.resolve(undefined);
      return startPromise;
    }
    startPromise = starting.then(
      () => undefined,
      () => handleStartupFailure(revision)
    );
    return startPromise;
  };

  const close = () => {
    if (closed) return undefined;
    closed = true;
    snapshotRevision += 1;
    const stop = unsubscribe;
    unsubscribe = undefined;
    try {
      stop?.();
    } catch {
      // Subscription cleanup is best effort during renderer teardown.
    }
    closeDomainOwner();
    try {
      startupOwner.close();
    } catch {
      // The renderer lifecycle is already closed even if native cleanup fails.
    }
    return undefined;
  };

  return Object.freeze({ close, start });
}
