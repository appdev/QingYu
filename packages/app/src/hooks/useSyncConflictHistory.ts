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
  const translateRef = useRef(translate);
  translateRef.current = translate;

  useEffect(() => {
    let active = true;
    let cleanup: (() => unknown) | null = null;
    let baselineReady = false;
    let activeRepositoryId: string | null = null;
    const bufferedStatuses = new Map<string, DejavuRepositoryStatus>();
    let reconciliation = Promise.resolve();
    const applyHistory = (records: readonly SyncConflictRecord[], notify: boolean) => {
      if (!active) return;
      const next = historyRecords(records);
      for (const conflict of next) {
        if (noticedConflictIds.has(conflict.conflictId)) continue;
        noticedConflictIds.add(conflict.conflictId);
        if (!notify) continue;
        showAppToast({
          id: `sync-conflict-${conflict.conflictId}`,
          message: translateRef.current("sync.conflict.notice"),
          status: "success",
          surface: "notice"
        });
      }
      setEntries(next);
    };
    const reconcilePersistedHistory = (notify: boolean) => {
      const repositoryId = activeRepositoryId;
      if (!repositoryId) return Promise.resolve();
      return getAppRuntime().syncConfig.listDejavuConflictHistory({ repositoryId }).then((records) => {
        if (!active || repositoryId !== activeRepositoryId) return;
        applyHistory(records, notify);
      }).catch(() => {});
    };
    const handleStatus = ({ payload }: { payload: DejavuRepositoryStatus }) => {
      if (!active) return;
      if (!baselineReady) {
        bufferedStatuses.set(payload.repositoryId, payload);
        return;
      }
      if (payload.repositoryId !== activeRepositoryId) return;
      reconciliation = reconciliation.then(() => reconcilePersistedHistory(true));
    };
    const install = async () => {
      if (!notesRoot) {
        setEntries([]);
        setRepositoryId(null);
        setLoading(false);
        return;
      }
      setLoading(true);
      try {
        if (getAppRuntime().events.isAvailable()) {
          try {
            cleanup = await getAppRuntime().events.listen<DejavuRepositoryStatus>(
              dejavuSyncStatusChangedEvent,
              handleStatus
            );
            if (!active) return cleanup();
          } catch {
            cleanup = null;
          }
        }
        const status = await getAppRuntime().syncConfig.loadRepositoryStatus({ notesRoot });
        if (!active) return;
        const nextRepositoryId = status?.repositoryId ?? null;
        const initial = nextRepositoryId
          ? await getAppRuntime().syncConfig.listDejavuConflictHistory({ repositoryId: nextRepositoryId })
          : [];
        if (!active) return;
        const nextConflicts = historyRecords(initial);
        activeRepositoryId = nextRepositoryId;
        setRepositoryId(nextRepositoryId);
        applyHistory(nextConflicts, false);
        baselineReady = true;
        const buffered = nextRepositoryId ? bufferedStatuses.has(nextRepositoryId) : false;
        bufferedStatuses.clear();
        await reconcilePersistedHistory(buffered);
      } catch {
        if (!active) return;
        activeRepositoryId = null;
        baselineReady = true;
        bufferedStatuses.clear();
        setRepositoryId(null);
        setEntries([]);
      } finally {
        if (active) setLoading(false);
      }

    };
    install().catch(() => {});
    return () => {
      active = false;
      bufferedStatuses.clear();
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
