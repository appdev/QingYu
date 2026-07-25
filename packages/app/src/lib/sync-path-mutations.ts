import {
  syncMutationIntersectsGuardedPaths,
  type SyncPathMutation
} from "./sync-path-events";

export type SyncPathMutationLease = {
  release: () => void;
};

type InFlightMutation = {
  finished: Promise<void>;
  mutation: SyncPathMutation;
  resolve: () => void;
};

export class SyncPathMutationRegistry {
  private readonly blockedRequests = new Map<string, ReadonlySet<string>>();
  private readonly inFlight = new Map<number, InFlightMutation>();
  private nextLeaseId = 1;

  isBlocked(mutation: SyncPathMutation) {
    return [...this.blockedRequests.values()].some((paths) => (
      syncMutationIntersectsGuardedPaths(mutation, paths)
    ));
  }

  acquire(mutation: SyncPathMutation): SyncPathMutationLease | null {
    if (this.isBlocked(mutation)) return null;

    const leaseId = this.nextLeaseId;
    this.nextLeaseId += 1;
    let resolve = () => {};
    const finished = new Promise<void>((nextResolve) => {
      resolve = nextResolve;
    });
    this.inFlight.set(leaseId, { finished, mutation, resolve });
    let released = false;

    return {
      release: () => {
        if (released) return;
        released = true;
        const active = this.inFlight.get(leaseId);
        this.inFlight.delete(leaseId);
        active?.resolve();
      }
    };
  }

  async prepare(requestId: string, paths: ReadonlySet<string>) {
    this.blockedRequests.set(requestId, new Set(paths));
    const affected = [...this.inFlight.values()]
      .filter(({ mutation }) => syncMutationIntersectsGuardedPaths(mutation, paths))
      .map(({ finished }) => finished);
    await Promise.all(affected);
  }

  release(requestId: string) {
    this.blockedRequests.delete(requestId);
  }

  clearRequests() {
    this.blockedRequests.clear();
  }
}
