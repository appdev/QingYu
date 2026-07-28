import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { I18nKey } from "@markra/shared";
import { showAppToast } from "../lib/app-toast";
import type { ConflictVersions, DejavuRepositoryStatus, SyncConflictRecord } from "../lib/sync-config";
import { dejavuSyncStatusChangedEvent } from "../lib/sync-config-events";
import { getAppRuntime } from "../runtime";

export { dejavuSyncStatusChangedEvent };

const noticedConflictIds = new Set<string>();

export function resetSyncConflictHistoryNoticeStateForTests() {
  noticedConflictIds.clear();
}

function historyRecords(records: readonly SyncConflictRecord[]) {
  const unique = new Map<string, SyncConflictRecord>();
  for (const record of records) {
    unique.set(record.conflictId, record);
  }
  return Array.from(unique.values());
}

export type SyncConflictHistoryController = {
  entries: readonly SyncConflictRecord[];
  loading: boolean;
  repositoryId: string | null;
  read: (conflict: SyncConflictRecord) => Promise<ConflictVersions>;
};

export function useSyncConflictHistory({
  notesRoot,
  translate
}: {
  notesRoot: string | null;
  translate: (key: I18nKey) => string;
}): SyncConflictHistoryController {
  const [entries, setEntries] = useState<SyncConflictRecord[]>([]);
  const [loading, setLoading] = useState(Boolean(notesRoot));
  const [repositoryId, setRepositoryId] = useState<string | null>(null);
  const repositoryIdRef = useRef<string | null>(null);
  const translateRef = useRef(translate);
  repositoryIdRef.current = repositoryId;
  translateRef.current = translate;

  useEffect(() => {
    let active = true;
    let cleanup: (() => unknown) | null = null;
    const install = async () => {
      if (!notesRoot) {
        setEntries([]);
        setRepositoryId(null);
        setLoading(false);
        return;
      }
      setLoading(true);
      try {
        const status = await getAppRuntime().syncConfig.loadRepositoryStatus({ notesRoot });
        if (!active) return;
        const nextRepositoryId = status?.repositoryId ?? null;
        const initial = nextRepositoryId
          ? await getAppRuntime().syncConfig.listDejavuConflictHistory({ repositoryId: nextRepositoryId })
          : [];
        if (!active) return;
        const nextConflicts = historyRecords(initial);
        nextConflicts.forEach((conflict) => noticedConflictIds.add(conflict.conflictId));
        repositoryIdRef.current = nextRepositoryId;
        setRepositoryId(nextRepositoryId);
        setEntries(nextConflicts);
      } catch {
        if (!active) return;
        repositoryIdRef.current = null;
        setRepositoryId(null);
        setEntries([]);
      } finally {
        if (active) setLoading(false);
      }

      if (!getAppRuntime().events.isAvailable()) return;
      cleanup = await getAppRuntime().events.listen<DejavuRepositoryStatus>(
        dejavuSyncStatusChangedEvent,
        ({ payload }) => {
          if (!active || payload.repositoryId !== repositoryIdRef.current) return;
          const next = historyRecords(payload.conflicts);
          for (const conflict of next) {
            if (noticedConflictIds.has(conflict.conflictId)) continue;
            noticedConflictIds.add(conflict.conflictId);
            showAppToast({
              id: `sync-conflict-${conflict.conflictId}`,
              message: translateRef.current("sync.conflict.notice"),
              status: "success",
              surface: "notice"
            });
          }
          setEntries(next);
        }
      );
      if (!active) cleanup();
    };
    install().catch(() => {});
    return () => {
      active = false;
      cleanup?.();
    };
  }, [notesRoot]);

  const read = useCallback((conflict: SyncConflictRecord) => (
    getAppRuntime().syncConfig.readDejavuConflictHistory({
      conflictId: conflict.conflictId,
      repositoryId: conflict.repositoryId
    })
  ), []);

  return useMemo(() => ({
    entries,
    loading,
    read,
    repositoryId
  }), [entries, loading, read, repositoryId]);
}
