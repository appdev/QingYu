import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { I18nKey } from "@markra/shared";
import { showAppToast } from "../lib/app-toast";
import { normalizeComparablePath } from "../lib/path-move";
import type {
  ConflictResolution,
  ConflictVersions,
  DejavuRepositoryStatus,
  SyncConflictRecord
} from "../lib/sync-config";
import { getAppRuntime } from "../runtime";

export const dejavuSyncStatusChangedEvent = "qingyu://dejavu-sync-status-changed";

const noticedConflictIds = new Set<string>();

export function resetSyncConflictNoticeStateForTests() {
  noticedConflictIds.clear();
}

function unresolved(records: readonly SyncConflictRecord[]) {
  const unique = new Map<string, SyncConflictRecord>();
  for (const record of records) {
    if (record.resolution === null) unique.set(record.conflictId, record);
  }
  return Array.from(unique.values());
}

export function conflictRelativePath(notesRoot: string | null, documentPath: string | null) {
  const root = normalizeComparablePath(notesRoot);
  const document = normalizeComparablePath(documentPath);
  if (!root || !document || root === document || !document.startsWith(`${root}/`)) return null;
  return document.slice(root.length + 1);
}

export type SyncConflictsController = {
  conflicts: readonly SyncConflictRecord[];
  loading: boolean;
  repositoryId: string | null;
  conflictForPath: (documentPath: string | null) => SyncConflictRecord | null;
  read: (conflict: SyncConflictRecord) => Promise<ConflictVersions>;
  resolve: (
    conflict: SyncConflictRecord,
    resolution: ConflictResolution
  ) => Promise<unknown>;
};

export function useSyncConflicts({
  notesRoot,
  translate
}: {
  notesRoot: string | null;
  translate: (key: I18nKey) => string;
}): SyncConflictsController {
  const [conflicts, setConflicts] = useState<SyncConflictRecord[]>([]);
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
        setConflicts([]);
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
          ? await getAppRuntime().syncConfig.listConflicts({ repositoryId: nextRepositoryId })
          : [];
        if (!active) return;
        const nextConflicts = unresolved(initial);
        nextConflicts.forEach((conflict) => noticedConflictIds.add(conflict.conflictId));
        repositoryIdRef.current = nextRepositoryId;
        setRepositoryId(nextRepositoryId);
        setConflicts(nextConflicts);
      } catch {
        if (!active) return;
        repositoryIdRef.current = null;
        setRepositoryId(null);
        setConflicts([]);
      } finally {
        if (active) setLoading(false);
      }

      if (!getAppRuntime().events.isAvailable()) return;
      cleanup = await getAppRuntime().events.listen<DejavuRepositoryStatus>(
        dejavuSyncStatusChangedEvent,
        ({ payload }) => {
          if (!active || payload.repositoryId !== repositoryIdRef.current) return;
          const next = unresolved(payload.conflicts);
          for (const conflict of next) {
            if (noticedConflictIds.has(conflict.conflictId)) continue;
            noticedConflictIds.add(conflict.conflictId);
            showAppToast({
              id: `sync-conflict-${conflict.conflictId}`,
              message: translateRef.current("sync.conflict.notice"),
              status: "error",
              surface: "notice"
            });
          }
          setConflicts(next);
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

  const conflictForPath = useCallback((documentPath: string | null) => {
    const relativePath = conflictRelativePath(notesRoot, documentPath);
    if (!relativePath) return null;
    const comparableRelative = normalizeComparablePath(relativePath);
    return conflicts.find((conflict) => (
      normalizeComparablePath(conflict.relativePath) === comparableRelative
    )) ?? null;
  }, [conflicts, notesRoot]);

  const read = useCallback((conflict: SyncConflictRecord) => (
    getAppRuntime().syncConfig.readConflict({
      conflictId: conflict.conflictId,
      repositoryId: conflict.repositoryId
    })
  ), []);

  const resolve = useCallback(async (
    conflict: SyncConflictRecord,
    resolution: ConflictResolution
  ) => {
    const accepted = await getAppRuntime().syncConfig.resolveConflict({
      conflictId: conflict.conflictId,
      repositoryId: conflict.repositoryId,
      resolution
    });
    setConflicts((current) => current.filter((record) => record.conflictId !== conflict.conflictId));
    return accepted;
  }, []);

  return useMemo(() => ({
    conflictForPath,
    conflicts,
    loading,
    read,
    repositoryId,
    resolve
  }), [conflictForPath, conflicts, loading, read, repositoryId, resolve]);
}
