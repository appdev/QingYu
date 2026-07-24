import { useCallback, useEffect, useRef, useState } from "react";
import type { AppMcpRuntime } from "../runtime";
import { isMcpRevisionConflict, type McpConfig, type McpSettingsSnapshot } from "../lib/mcp";
import { listenMcpPolicyChanged, listenMcpRuntimeChanged } from "../lib/settings/settings-events";

export function useMcpSettings(runtime: AppMcpRuntime) {
  const [snapshot, setSnapshot] = useState<McpSettingsSnapshot | null>(null);
  const [loading, setLoading] = useState(runtime.policyAvailable);
  const [error, setError] = useState<string | null>(null);
  const active = useRef(false);
  const requestGeneration = useRef(0);

  const reload = useCallback(async () => {
    if (!runtime.policyAvailable || !active.current) return;
    const generation = requestGeneration.current + 1;
    requestGeneration.current = generation;
    setLoading(true);
    try {
      const current = await runtime.getSettings();
      if (!active.current || requestGeneration.current !== generation) return;
      setSnapshot(current);
      setError(null);
    } catch (cause) {
      if (!active.current || requestGeneration.current !== generation) return;
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (active.current && requestGeneration.current === generation) setLoading(false);
    }
  }, [runtime]);

  useEffect(() => {
    if (!runtime.policyAvailable) {
      active.current = false;
      requestGeneration.current += 1;
      setLoading(false);
      return;
    }
    active.current = true;
    let cancelled = false;
    let cleanups: Array<() => unknown> = [];
    let listenersReady = false;

    const onChange = () => {
      if (cancelled) return;
      if (!listenersReady) return;
      reload().catch(() => {});
    };

    Promise.all([
      listenMcpPolicyChanged(onChange).catch(() => null),
      listenMcpRuntimeChanged(onChange).catch(() => null)
    ]).then((installations) => {
      const installedCleanups = installations.filter(
        (cleanup): cleanup is () => unknown => cleanup !== null
      );
      if (cancelled) {
        for (const cleanup of installedCleanups) cleanup();
        return;
      }
      if (installedCleanups.length !== installations.length) {
        for (const cleanup of installedCleanups) cleanup();
        reload().catch(() => {});
        return;
      }
      cleanups = installedCleanups;
      listenersReady = true;
      reload().catch(() => {});
    });

    return () => {
      cancelled = true;
      active.current = false;
      requestGeneration.current += 1;
      for (const cleanup of cleanups) cleanup();
    };
  }, [reload, runtime.policyAvailable]);

  const updateConfig = useCallback(async (config: McpConfig) => {
    if (!snapshot || !active.current) return null;
    const generation = requestGeneration.current + 1;
    requestGeneration.current = generation;
    try {
      const updated = await runtime.updateSettings({
        config,
        expectedRevision: snapshot.revision
      });
      if (!active.current || requestGeneration.current !== generation) return null;
      setSnapshot(updated);
      setError(null);
      return updated;
    } catch (cause) {
      if (!active.current || requestGeneration.current !== generation) return null;
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
      if (isMcpRevisionConflict(message)) {
        const conflictReloadGeneration = requestGeneration.current + 1;
        await reload();
        if (!active.current || requestGeneration.current !== conflictReloadGeneration) return null;
        setError(message);
      }
      return null;
    }
  }, [reload, runtime, snapshot]);

  return { error, loading, reload, setSnapshot, snapshot, updateConfig };
}
