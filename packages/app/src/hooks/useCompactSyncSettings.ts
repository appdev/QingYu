import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  SyncConfigDocument,
  SyncConfigLoadResult,
  SyncStatus
} from "../lib/sync-config";
import { listenSyncStatusChanged } from "../lib/sync-config-events";
import { getAppRuntime } from "../runtime";
import {
  useSyncSettingsSession,
  type SyncSettingsSession
} from "./useSyncSettingsSession";

export type CompactSyncSettingsController = SyncSettingsSession & {
  available: boolean;
  configDocument: SyncConfigDocument | null;
  primaryRoot: string | null;
  status: SyncStatus | null;
  syncRunning: boolean;
};

export type UseCompactSyncSettingsInput = {
  available: boolean;
  observedLoadResult: SyncConfigLoadResult | null;
  primaryRoot: string | null;
  runImmediate?: () => Promise<unknown>;
  shouldBegin: boolean;
};

function documentFromResult(result: SyncConfigLoadResult | null): SyncConfigDocument | null {
  if (result?.status !== "loaded") return null;
  return {
    config: result.config,
    configured: result.configured,
    issues: result.issues,
    readiness: result.readiness,
    revision: result.revision
  };
}

function statusMatches(status: SyncStatus | null, primaryRoot: string | null, revision: string | null) {
  return Boolean(
    status &&
    primaryRoot &&
    revision &&
    status.notesRoot === primaryRoot &&
    status.revision === revision
  );
}

export function useCompactSyncSettings({
  available,
  observedLoadResult,
  primaryRoot,
  runImmediate,
  shouldBegin
}: UseCompactSyncSettingsInput): CompactSyncSettingsController {
  const session = useSyncSettingsSession({ primaryRoot });
  const loadResult = session.loadResult ?? observedLoadResult;
  const configDocument = documentFromResult(loadResult);
  const revision = configDocument?.revision ?? null;
  const [statusView, setStatusView] = useState<SyncStatus | null>(null);
  const statusGenerationRef = useRef(0);
  const identityRef = useRef({ primaryRoot, revision });
  identityRef.current = { primaryRoot, revision };

  const reloadStatus = useCallback(async () => {
    const generation = statusGenerationRef.current + 1;
    statusGenerationRef.current = generation;
    if (!available || !primaryRoot || !revision) {
      setStatusView(null);
      return;
    }
    try {
      const status = await getAppRuntime().syncConfig.loadStatus();
      if (statusGenerationRef.current !== generation) return;
      setStatusView(statusMatches(status, primaryRoot, revision) ? status : null);
    } catch {
      // Keep the last identity-matched status when a refresh is temporarily unavailable.
    }
  }, [available, primaryRoot, revision]);

  useEffect(() => {
    if (!available || !shouldBegin) return;
    session.begin().catch(() => {});
  }, [available, session.begin, shouldBegin]);

  useEffect(() => {
    setStatusView(null);
    const invalidations = getAppRuntime().kernel.invalidations;
    const unsubscribe = available && invalidations.available
      ? invalidations.subscribe((notice) => {
          if (!notice.scopes.includes("sync-status")) return;
          reloadStatus().catch(() => {});
        })
      : null;
    reloadStatus().catch(() => {});
    return () => {
      statusGenerationRef.current += 1;
      unsubscribe?.();
    };
  }, [available, reloadStatus]);

  useEffect(() => {
    if (!available) return;
    let cancelled = false;
    let cleanup: (() => unknown) | null = null;
    listenSyncStatusChanged(({ notesRoot, revision: eventRevision, status }) => {
      const identity = identityRef.current;
      if (
        cancelled ||
        notesRoot !== identity.primaryRoot ||
        eventRevision !== identity.revision ||
        !statusMatches(status, identity.primaryRoot, identity.revision)
      ) return;
      statusGenerationRef.current += 1;
      setStatusView(status);
    }).then((stop) => {
      if (!cancelled) cleanup = stop;
      else stop();
    }).catch(() => {});
    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [available]);

  const status = statusMatches(statusView, primaryRoot, revision) ? statusView : null;
  return useMemo(() => ({
    ...session,
    available,
    configDocument,
    loadResult,
    primaryRoot,
    runImmediate: runImmediate ?? session.runImmediate,
    status,
    syncRunning: status?.completionState === "attempting"
  }), [
    available,
    configDocument,
    loadResult,
    primaryRoot,
    runImmediate,
    session,
    status
  ]);
}
