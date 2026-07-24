import { useCallback, useEffect, useRef, useState } from "react";
import { pathNameFromPath, type I18nKey } from "@markra/shared";
import { requestPrimaryCloudNotebookRestore } from "../lib/cloud-notebook-restore-events";
import { getAppRuntime, type RemoteNotebookCatalogEntry } from "../runtime";
import type { CompactSyncSettingsController } from "./useCompactSyncSettings";

export type SettingsRemoteNotebookDialogController = {
  currentNotebookName: string | null;
  entries: readonly RemoteNotebookCatalogEntry[];
  error: string | null;
  loading: boolean;
  open: boolean;
  openDialog: () => Promise<unknown>;
  cancel: () => unknown;
  refresh: () => Promise<unknown>;
  restore: (remoteName: string) => Promise<unknown>;
};

export type UseSettingsRemoteNotebookDialogInput = {
  onSessionFailure: () => unknown;
  primaryRoot: string | null;
  syncSession: Pick<CompactSyncSettingsController, "begin" | "end">;
  translate: (key: I18nKey) => string;
};

export function useSettingsRemoteNotebookDialog({
  onSessionFailure,
  primaryRoot,
  syncSession,
  translate
}: UseSettingsRemoteNotebookDialogInput): SettingsRemoteNotebookDialogController {
  const [entries, setEntries] = useState<RemoteNotebookCatalogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [open, setOpen] = useState(false);
  const [resumeNotebookName, setResumeNotebookName] = useState<string | null>(null);
  const catalogGenerationRef = useRef(0);
  const openRef = useRef(false);
  const openPromiseRef = useRef<Promise<unknown> | null>(null);
  const restoreAbortControllerRef = useRef<AbortController | null>(null);
  const revisionRef = useRef<string | null>(null);
  const currentNotebookName = primaryRoot ? pathNameFromPath(primaryRoot) : null;

  const restartSession = useCallback(async () => {
    try {
      await syncSession.begin();
    } catch {
      onSessionFailure();
    }
  }, [onSessionFailure, syncSession.begin]);

  const loadCatalog = useCallback(async (generation: number) => {
    try {
      const loadResult = await getAppRuntime().syncConfig.load();
      if (
        loadResult.status !== "loaded"
        || loadResult.configured !== true
        || loadResult.revision === null
      ) {
        throw new Error("Cloud notebook catalog is unavailable");
      }

      revisionRef.current = loadResult.revision;
      const nextEntries = await getAppRuntime().syncConfig.listNotebooks({
        revision: loadResult.revision
      });
      if (catalogGenerationRef.current !== generation) return;

      setEntries(nextEntries);
      setError(null);
    } catch {
      if (catalogGenerationRef.current !== generation) return;
      setEntries([]);
      setError(translate("notebooks.remote.refreshError"));
    } finally {
      if (catalogGenerationRef.current === generation) setLoading(false);
    }
  }, [translate]);

  const openDialog = useCallback(() => {
    const pendingOpen = openPromiseRef.current;
    if (pendingOpen) return pendingOpen;

    let request!: Promise<unknown>;
    request = (async () => {
      try {
        await syncSession.end("catalog-handoff");
      } catch {
        onSessionFailure();
        return;
      }

      const generation = ++catalogGenerationRef.current;
      openRef.current = true;
      setEntries([]);
      setError(null);
      setLoading(true);
      setOpen(true);
      await loadCatalog(generation);
    })().finally(() => {
      if (openPromiseRef.current === request) openPromiseRef.current = null;
    });
    openPromiseRef.current = request;
    return request;
  }, [loadCatalog, onSessionFailure, syncSession.end]);

  const cancel = useCallback(() => {
    catalogGenerationRef.current += 1;
    openRef.current = false;
    setError(null);
    setLoading(false);
    setOpen(false);
    restartSession().catch(() => {});
  }, [restartSession]);

  const refresh = useCallback(async () => {
    if (!openRef.current) return;

    const generation = ++catalogGenerationRef.current;
    setError(null);
    setLoading(true);
    await loadCatalog(generation);
  }, [loadCatalog]);

  const restore = useCallback(async (remoteName: string) => {
    const revision = revisionRef.current;
    if (!revision) throw new Error("Cloud notebook restore failed");

    restoreAbortControllerRef.current?.abort();
    const abortController = new AbortController();
    restoreAbortControllerRef.current = abortController;
    try {
      const succeeded = await requestPrimaryCloudNotebookRestore({
        remoteName,
        revision,
        signal: abortController.signal
      });
      if (!succeeded) throw new Error("Cloud notebook restore failed");

      catalogGenerationRef.current += 1;
      openRef.current = false;
      setLoading(false);
      setOpen(false);
      if (currentNotebookName === remoteName) {
        await restartSession();
      } else {
        setResumeNotebookName(remoteName);
      }
    } finally {
      if (restoreAbortControllerRef.current === abortController) {
        restoreAbortControllerRef.current = null;
      }
    }
  }, [currentNotebookName, restartSession]);

  useEffect(() => {
    if (resumeNotebookName === null || currentNotebookName !== resumeNotebookName) return;
    setResumeNotebookName(null);
    restartSession().catch(() => {});
  }, [currentNotebookName, restartSession, resumeNotebookName]);

  useEffect(() => () => {
    catalogGenerationRef.current += 1;
    restoreAbortControllerRef.current?.abort();
  }, []);

  return {
    cancel,
    currentNotebookName,
    entries,
    error,
    loading,
    open,
    openDialog,
    refresh,
    restore
  };
}
