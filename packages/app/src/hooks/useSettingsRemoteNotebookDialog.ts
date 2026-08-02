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
  leaveSync: (reason: "category-leave" | "window-close") => Promise<unknown>;
  open: boolean;
  openDialog: () => Promise<unknown>;
  cancel: () => unknown;
  refresh: () => Promise<unknown>;
  resumeSync: () => Promise<unknown>;
  restore: (entry: RemoteNotebookCatalogEntry) => Promise<unknown>;
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
  const currentNotebookName = primaryRoot ? pathNameFromPath(primaryRoot) : null;
  const currentNotebookNameRef = useRef(currentNotebookName);
  currentNotebookNameRef.current = currentNotebookName;
  const primaryRootRef = useRef(primaryRoot);
  primaryRootRef.current = primaryRoot;
  const mountedRef = useRef(true);
  const openRef = useRef(false);
  const openPromiseRef = useRef<Promise<unknown> | null>(null);
  const resumeWaiterRef = useRef<{ generation: number; remoteName: string } | null>(null);
  const restoreAbortControllerRef = useRef<AbortController | null>(null);
  const revisionRef = useRef<string | null>(null);
  const sessionEndedForDialogRef = useRef(false);
  const sessionTransitionRef = useRef<Promise<unknown>>(Promise.resolve(undefined));
  const transactionGenerationRef = useRef(0);

  const enqueueSessionTransition = useCallback((transition: () => Promise<unknown>) => {
    const request = sessionTransitionRef.current.then(transition, transition);
    sessionTransitionRef.current = request.then(
      () => undefined,
      () => undefined
    );
    return request;
  }, []);

  const resumeSync = useCallback(() => enqueueSessionTransition(() => mountedRef.current
    ? syncSession.begin()
    : Promise.resolve(undefined)), [enqueueSessionTransition, syncSession.begin]);

  const endSync = useCallback((
    reason: Parameters<CompactSyncSettingsController["end"]>[0]
  ) => enqueueSessionTransition(() => mountedRef.current
    ? syncSession.end(reason)
    : Promise.resolve(undefined)), [enqueueSessionTransition, syncSession.end]);

  const restartSession = useCallback(async () => {
    try {
      await resumeSync();
      return true;
    } catch {
      onSessionFailure();
      return false;
    }
  }, [onSessionFailure, resumeSync]);

  const endSession = useCallback(async (
    reason: Parameters<CompactSyncSettingsController["end"]>[0]
  ) => {
    try {
      await endSync(reason);
      return true;
    } catch {
      onSessionFailure();
      return false;
    }
  }, [endSync, onSessionFailure]);

  const invalidateTransaction = useCallback(() => {
    const generation = transactionGenerationRef.current + 1;
    transactionGenerationRef.current = generation;
    openPromiseRef.current = null;
    resumeWaiterRef.current = null;
    restoreAbortControllerRef.current?.abort();
    restoreAbortControllerRef.current = null;
    return generation;
  }, []);

  const closeDialog = useCallback(() => {
    openRef.current = false;
    revisionRef.current = null;
    if (!mountedRef.current) return;
    setEntries([]);
    setError(null);
    setLoading(false);
    setOpen(false);
  }, []);

  const leaveSync = useCallback((reason: "category-leave" | "window-close") => {
    invalidateTransaction();
    sessionEndedForDialogRef.current = false;
    closeDialog();
    return endSync(reason);
  }, [closeDialog, endSync, invalidateTransaction]);

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
      if (!mountedRef.current || transactionGenerationRef.current !== generation) return;

      revisionRef.current = loadResult.revision;
      const nextEntries = await getAppRuntime().syncConfig.listNotebooks({
        revision: loadResult.revision
      });
      if (!mountedRef.current || transactionGenerationRef.current !== generation) return;

      setEntries(nextEntries.filter((entry) => (
        entry.provider !== "s3" || getAppRuntime().features.dejavuSync
      )));
      setError(null);
    } catch {
      if (!mountedRef.current || transactionGenerationRef.current !== generation) return;
      setEntries([]);
      setError(translate("notebooks.remote.refreshError"));
    } finally {
      if (mountedRef.current && transactionGenerationRef.current === generation) {
        setLoading(false);
      }
    }
  }, [translate]);

  const openDialog = useCallback(() => {
    const pendingOpen = openPromiseRef.current;
    if (pendingOpen) return pendingOpen;

    const generation = invalidateTransaction();
    let request!: Promise<unknown>;
    request = (async () => {
      const editingEnded = await endSession("catalog-handoff");
      if (
        !editingEnded
        || !mountedRef.current
        || transactionGenerationRef.current !== generation
      ) return;

      sessionEndedForDialogRef.current = true;
      openRef.current = true;
      revisionRef.current = null;
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
  }, [endSession, invalidateTransaction, loadCatalog]);

  const cancel = useCallback(() => {
    const generation = invalidateTransaction();
    const shouldRestartSession = sessionEndedForDialogRef.current;
    sessionEndedForDialogRef.current = false;
    closeDialog();
    if (!shouldRestartSession) return Promise.resolve(undefined);

    return restartSession().then((restarted) => {
      if (!restarted && transactionGenerationRef.current === generation) {
        sessionEndedForDialogRef.current = true;
      }
    });
  }, [closeDialog, invalidateTransaction, restartSession]);

  const refresh = useCallback(async () => {
    if (!openRef.current) return;

    const generation = invalidateTransaction();
    revisionRef.current = null;
    setError(null);
    setLoading(true);
    await loadCatalog(generation);
  }, [invalidateTransaction, loadCatalog]);

  const restore = useCallback(async (entry: RemoteNotebookCatalogEntry) => {
    if (!entry.available) throw new Error("Cloud notebook restore failed");
    const revision = revisionRef.current;
    if (!revision) throw new Error("Cloud notebook restore failed");
    const notesRoot = primaryRootRef.current;
    if (entry.provider === "s3" && !getAppRuntime().features.dejavuSync) {
      throw new Error("Cloud notebook restore failed");
    }
    if (entry.provider === "s3" && !notesRoot) {
      throw new Error("Cloud notebook restore failed");
    }

    const generation = invalidateTransaction();
    const abortController = new AbortController();
    restoreAbortControllerRef.current = abortController;
    try {
      const succeeded = await requestPrimaryCloudNotebookRestore(entry.provider === "s3"
        ? {
            displayName: entry.displayName,
            notesRoot: notesRoot!,
            provider: "s3",
            repositoryId: entry.repositoryId,
            revision,
            signal: abortController.signal
          }
        : {
            provider: "webdav",
            remoteName: entry.name,
            revision,
            signal: abortController.signal
          });
      if (!succeeded) throw new Error("Cloud notebook restore failed");
      if (!mountedRef.current || transactionGenerationRef.current !== generation) return;

      openRef.current = false;
      setLoading(false);
      setOpen(false);
      if (entry.provider === "s3" || currentNotebookNameRef.current === entry.name) {
        sessionEndedForDialogRef.current = false;
        const restarted = await restartSession();
        if (!restarted && transactionGenerationRef.current === generation) {
          sessionEndedForDialogRef.current = true;
        }
      } else {
        resumeWaiterRef.current = { generation, remoteName: entry.name };
      }
    } finally {
      if (restoreAbortControllerRef.current === abortController) {
        restoreAbortControllerRef.current = null;
      }
    }
  }, [invalidateTransaction, restartSession]);

  useEffect(() => {
    const waiter = resumeWaiterRef.current;
    if (
      waiter === null
      || waiter.generation !== transactionGenerationRef.current
      || currentNotebookName !== waiter.remoteName
    ) return;

    resumeWaiterRef.current = null;
    sessionEndedForDialogRef.current = false;
    restartSession().then((restarted) => {
      if (!restarted && transactionGenerationRef.current === waiter.generation) {
        sessionEndedForDialogRef.current = true;
      }
    }).catch(() => {});
  }, [currentNotebookName, restartSession]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      transactionGenerationRef.current += 1;
      openPromiseRef.current = null;
      resumeWaiterRef.current = null;
      sessionEndedForDialogRef.current = false;
      restoreAbortControllerRef.current?.abort();
      restoreAbortControllerRef.current = null;
    };
  }, []);

  return {
    cancel,
    currentNotebookName,
    entries,
    error,
    leaveSync,
    loading,
    open,
    openDialog,
    refresh,
    resumeSync,
    restore
  };
}
