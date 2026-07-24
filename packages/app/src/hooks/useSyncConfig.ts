import { useCallback, useEffect, useRef, useState } from "react";
import type { SyncConfigDocument, SyncConfigLoadResult } from "../lib/sync-config";
import { listenSyncConfigChanged } from "../lib/sync-config-events";
import { getAppRuntime } from "../runtime";

export type SyncConfigLoadStatus = "error" | "idle" | "loading" | "loaded";

export type SyncConfigState = {
  appliedDocument: SyncConfigDocument | null;
  loadResult: SyncConfigLoadResult | null;
  reload: () => Promise<SyncConfigLoadResult | null>;
  status: SyncConfigLoadStatus;
};

function appliedDocument(result: SyncConfigLoadResult | null): SyncConfigDocument | null {
  if (result?.status !== "loaded") return null;
  return {
    config: result.config,
    configured: result.configured,
    issues: result.issues,
    readiness: result.readiness,
    revision: result.revision
  };
}

export function useSyncConfig({ active = true }: { active?: boolean } = {}): SyncConfigState {
  const [loadResult, setLoadResult] = useState<SyncConfigLoadResult | null>(null);
  const [status, setStatus] = useState<SyncConfigLoadStatus>("loading");
  const generationRef = useRef(0);
  const revisionRef = useRef<string | null>(null);
  const pendingRevisionRef = useRef<string | null>(null);

  const reload = useCallback(async () => {
    const generation = ++generationRef.current;
    setStatus("loading");
    try {
      const result = await getAppRuntime().syncConfig.load();
      if (generationRef.current !== generation) return null;
      const resultRevision = result.revision;
      revisionRef.current = resultRevision;
      setLoadResult(result);
      setStatus("loaded");
      if (pendingRevisionRef.current && pendingRevisionRef.current !== resultRevision) {
        pendingRevisionRef.current = null;
        return reload();
      }
      pendingRevisionRef.current = null;
      return result;
    } catch {
      if (generationRef.current === generation) {
        setLoadResult(null);
        setStatus("error");
      }
      return null;
    }
  }, []);

  useEffect(() => {
    let alive = true;
    let cleanup: (() => unknown) | null = null;
    if (!active) {
      generationRef.current += 1;
      revisionRef.current = null;
      pendingRevisionRef.current = null;
      setLoadResult(null);
      setStatus("idle");
      return;
    }
    const start = () => {
      if (alive) reload().catch(() => {});
    };
    listenSyncConfigChanged(({ revision }) => {
      if (!alive || revision === revisionRef.current || revision === pendingRevisionRef.current) return;
      pendingRevisionRef.current = revision;
      reload().catch(() => {});
    }).then((stop) => {
      if (!alive) return stop();
      cleanup = stop;
      start();
    }).catch(start);
    return () => {
      alive = false;
      generationRef.current += 1;
      cleanup?.();
    };
  }, [active, reload]);

  return { appliedDocument: appliedDocument(loadResult), loadResult, reload, status };
}
