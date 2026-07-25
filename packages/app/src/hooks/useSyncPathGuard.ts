import { useCallback, useEffect, useRef, useState } from "react";
import {
  absoluteSyncGuardPaths,
  guardedPathsForRequests,
  parseSyncPathGuardRelease,
  parseSyncPathGuardRequest,
  syncPathGuardReleaseEvent,
  syncPathGuardRequestEvent,
  type SyncPathGuardRequest
} from "../lib/sync-path-events";
import { normalizeComparablePath } from "../lib/path-move";
import { SyncPathMutationRegistry } from "../lib/sync-path-mutations";
import { getAppRuntime } from "../runtime";

type UseSyncPathGuardOptions = {
  enabled: boolean;
  mutationRegistry: SyncPathMutationRegistry;
  notesRoot: string | null;
  saveDirtyMarkdownPaths: (paths: readonly string[]) => Promise<boolean>;
};

const frontendPathFlushTimeoutMs = 14_000;

async function flushBeforeNativeTimeout(
  saveDirtyMarkdownPaths: () => Promise<boolean>
) {
  let timeoutId: number | null = null;
  try {
    return await Promise.race([
      saveDirtyMarkdownPaths(),
      new Promise<boolean>((resolve) => {
        timeoutId = window.setTimeout(() => resolve(false), frontendPathFlushTimeoutMs);
      })
    ]);
  } finally {
    if (timeoutId !== null) window.clearTimeout(timeoutId);
  }
}

export function useSyncPathGuard({
  enabled,
  mutationRegistry,
  notesRoot,
  saveDirtyMarkdownPaths
}: UseSyncPathGuardOptions) {
  const requestsRef = useRef(new Map<string, { request: SyncPathGuardRequest; paths: Set<string> }>());
  const pendingRef = useRef(new Map<string, SyncPathGuardRequest>());
  const pendingAcknowledgementsRef = useRef(new Map<string, SyncPathGuardRequest>());
  const mountedRef = useRef(true);
  const [guardedPaths, setGuardedPaths] = useState<ReadonlySet<string>>(new Set());

  const publishGuardedPaths = useCallback(() => {
    const requestPaths = new Map<string, ReadonlySet<string>>();
    requestsRef.current.forEach(({ paths }, requestId) => requestPaths.set(requestId, paths));
    setGuardedPaths(guardedPathsForRequests(requestPaths));
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    requestsRef.current.clear();
    pendingRef.current.clear();
    pendingAcknowledgementsRef.current.clear();
    mutationRegistry.clearRequests();
    setGuardedPaths(new Set());
    if (!enabled || !notesRoot || !getAppRuntime().events.isAvailable()) return;

    const runtime = getAppRuntime();
    let active = true;
    const cleanups: Array<() => unknown> = [];
    const install = async () => {
      const releaseCleanup = await runtime.events.listen(syncPathGuardReleaseEvent, (event) => {
        const input = event.payload as { requestId?: unknown };
        if (!active || typeof input?.requestId !== "string") return;
        const existing = requestsRef.current.get(input.requestId);
        const pending = pendingRef.current.get(input.requestId);
        const request = existing?.request ?? pending;
        if (!request || !parseSyncPathGuardRelease(event.payload, request)) return;

        requestsRef.current.delete(input.requestId);
        pendingRef.current.delete(input.requestId);
        pendingAcknowledgementsRef.current.delete(input.requestId);
        mutationRegistry.release(input.requestId);
        publishGuardedPaths();
      });
      if (!active) {
        releaseCleanup();
        return;
      }
      cleanups.push(releaseCleanup);

      const requestCleanup = await runtime.events.listen(syncPathGuardRequestEvent, async (event) => {
        const request = parseSyncPathGuardRequest(event.payload);
        if (
          !active ||
          !request ||
          normalizeComparablePath(request.notesRoot) !== normalizeComparablePath(notesRoot) ||
          requestsRef.current.has(request.requestId) ||
          pendingRef.current.has(request.requestId)
        ) {
          return;
        }
        pendingRef.current.set(request.requestId, request);
        const paths = absoluteSyncGuardPaths(request.notesRoot, request.relativePaths);
        let saved = false;
        try {
          saved = await flushBeforeNativeTimeout(async () => {
            await mutationRegistry.prepare(request.requestId, new Set(paths));
            if (!active || pendingRef.current.get(request.requestId) !== request) return false;
            return saveDirtyMarkdownPaths(paths);
          });
        } catch {
          saved = false;
        }
        pendingRef.current.delete(request.requestId);
        if (!active || !mountedRef.current || !saved) {
          mutationRegistry.release(request.requestId);
          return;
        }

        requestsRef.current.set(request.requestId, { request, paths: new Set(paths) });
        pendingAcknowledgementsRef.current.set(request.requestId, request);
        publishGuardedPaths();
      });
      if (!active) {
        requestCleanup();
        return;
      }
      cleanups.push(requestCleanup);
    };
    install().catch(() => {});

    return () => {
      active = false;
      cleanups.forEach((cleanup) => cleanup());
      requestsRef.current.clear();
      pendingRef.current.clear();
      pendingAcknowledgementsRef.current.clear();
      mutationRegistry.clearRequests();
    };
  }, [enabled, mutationRegistry, notesRoot, publishGuardedPaths, saveDirtyMarkdownPaths]);

  useEffect(() => {
    const pending = [...pendingAcknowledgementsRef.current.values()];
    pending.forEach((request) => {
      const guardedRequest = requestsRef.current.get(request.requestId);
      if (
        !mountedRef.current ||
        guardedRequest?.request !== request ||
        [...guardedRequest.paths].some((path) => !guardedPaths.has(path))
      ) {
        return;
      }

      pendingAcknowledgementsRef.current.delete(request.requestId);
      getAppRuntime().syncPathGuard.acknowledge({
        notesRoot: request.notesRoot,
        requestId: request.requestId
      }).catch(() => {
        if (!mountedRef.current || requestsRef.current.get(request.requestId)?.request !== request) return;
        requestsRef.current.delete(request.requestId);
        mutationRegistry.release(request.requestId);
        publishGuardedPaths();
      });
    });
  }, [guardedPaths, mutationRegistry, publishGuardedPaths]);

  return { guardedPaths };
}
