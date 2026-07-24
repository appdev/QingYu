import { useCallback, useEffect, useRef, useState } from "react";
import type { SyncConfigDocument, SyncRunResult } from "../lib/sync-config";
import { notebookNameFromRoot } from "../lib/sync-config";
import {
  listenNotebookSwitchRequested,
  type NotebookSwitchRequest
} from "../lib/notebook-switch-events";
import {
  getStoredRecentNotebooks,
  isValidManagedNotebookName,
  removeStoredRecentNotebook,
  saveStoredRecentNotebook
} from "../lib/settings/local-state";
import type { RecentNotebook } from "../lib/settings/recent-markdown";
import { getAppRuntime } from "../runtime";
import type { AppSyncCoordinator } from "./useAppSyncCoordinator";
import type { PrimaryWorkspaceController } from "./usePrimaryWorkspace";

export type NotebookSwitchCoordinator = {
  recentNotebooks: RecentNotebook[];
  removeRecentNotebook: (path: string) => Promise<unknown>;
  restoreDesktopNotebook: (
    remoteName: string,
    parentPath?: string
  ) => Promise<string | null>;
  restoreManagedNotebook: (remoteName: string) => Promise<string | null>;
  switchDesktopNotebook: (path?: string) => Promise<string | null>;
  switchManagedNotebook: (name: string) => Promise<string | null>;
  switching: boolean;
};

export type NotebookSwitchCoordinatorInput = {
  appSync: AppSyncCoordinator;
  configDocument: SyncConfigDocument | null;
  flushActiveDocument: () => Promise<unknown>;
  primaryRoot: string | null;
  primaryWindowOwner: boolean;
  primaryWorkspace: PrimaryWorkspaceController;
  trueMobile: boolean;
};

type MountedRootWaiter = {
  resolve: (mounted: boolean) => unknown;
  root: string;
};

type PendingDesktopNotebookSwitch = {
  request: NotebookSwitchRequest;
  settle: Array<(root: string | null) => unknown>;
};

type NotebookSwitchTransactionKind = "default" | "desktop-remote-restore";

function bootstrapResultMatches(
  result: SyncRunResult,
  notesRoot: string,
  revision: string
) {
  return result.notesRoot === notesRoot &&
    result.notebookName === notebookNameFromRoot(notesRoot) &&
    result.revision === revision;
}

export function useNotebookSwitchCoordinator({
  appSync,
  configDocument,
  flushActiveDocument,
  primaryRoot,
  primaryWindowOwner,
  primaryWorkspace,
  trueMobile
}: NotebookSwitchCoordinatorInput): NotebookSwitchCoordinator {
  const [switching, setSwitching] = useState(false);
  const [recentNotebooks, setRecentNotebooks] = useState<RecentNotebook[]>([]);
  const activeTransactionRef = useRef<Promise<string | null> | null>(null);
  const activeTransactionKindRef = useRef<NotebookSwitchTransactionKind | null>(null);
  const mountedRef = useRef(true);
  const mountedRootRef = useRef(primaryRoot);
  const mountedRootWaitersRef = useRef<MountedRootWaiter[]>([]);
  const recentNotebooksGenerationRef = useRef(0);
  const pendingRequestRef = useRef<PendingDesktopNotebookSwitch | null>(null);
  const desktopRequestInProgressRef = useRef(false);
  const requestPumpRef = useRef<Promise<unknown> | null>(null);
  const startDesktopNotebookSwitchPumpRef = useRef<() => unknown>(() => undefined);
  mountedRootRef.current = primaryRoot;

  useEffect(() => {
    const remaining: MountedRootWaiter[] = [];
    for (const waiter of mountedRootWaitersRef.current) {
      if (primaryRoot === waiter.root) waiter.resolve(true);
      else remaining.push(waiter);
    }
    mountedRootWaitersRef.current = remaining;
  }, [primaryRoot]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      for (const waiter of mountedRootWaitersRef.current) waiter.resolve(false);
      mountedRootWaitersRef.current = [];
      const pending = pendingRequestRef.current;
      pendingRequestRef.current = null;
      for (const settle of pending?.settle ?? []) settle(null);
    };
  }, []);

  useEffect(() => {
    let active = true;
    const generation = recentNotebooksGenerationRef.current + 1;
    recentNotebooksGenerationRef.current = generation;
    getStoredRecentNotebooks()
      .then((notebooks) => {
        if (active && recentNotebooksGenerationRef.current === generation) {
          setRecentNotebooks(notebooks);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const waitForMountedRoot = useCallback((root: string) => {
    if (!mountedRef.current) return Promise.resolve(false);
    if (mountedRootRef.current === root) return Promise.resolve(true);
    return new Promise<boolean>((resolve) => {
      mountedRootWaitersRef.current.push({ resolve, root });
    });
  }, []);

  const requestVisibleLaunchSync = useCallback(async () => {
    const document = configDocument;
    if (!document?.config.enabled || document.readiness !== "ready") return;
    await appSync.run("app-launch", document.revision).catch(() => null);
  }, [appSync, configDocument]);

  const runTransaction = useCallback((
    operation: () => Promise<string | null>,
    kind: NotebookSwitchTransactionKind = "default"
  ) => {
    if (!primaryWindowOwner || activeTransactionRef.current) return Promise.resolve(null);
    activeTransactionKindRef.current = kind;
    setSwitching(true);
    const transaction = operation().finally(() => {
      if (activeTransactionRef.current === transaction) {
        activeTransactionRef.current = null;
        activeTransactionKindRef.current = null;
      }
      if (mountedRef.current) setSwitching(false);
    });
    activeTransactionRef.current = transaction;
    return transaction;
  }, [primaryWindowOwner]);

  const finishCommittedRoot = useCallback(async (
    root: string,
    releaseBarrier: () => Promise<unknown>
  ) => {
    const mounted = await waitForMountedRoot(root);
    if (!mounted) return null;
    if (primaryWindowOwner) {
      const recentGeneration = recentNotebooksGenerationRef.current + 1;
      recentNotebooksGenerationRef.current = recentGeneration;
      const notebooks = await saveStoredRecentNotebook({ name: notebookNameFromRoot(root), path: root }).catch(() => null);
      if (
        notebooks &&
        mountedRef.current &&
        recentNotebooksGenerationRef.current === recentGeneration
      ) {
        setRecentNotebooks(notebooks);
      }
    }
    await releaseBarrier();
    await requestVisibleLaunchSync();
    return root;
  }, [primaryWindowOwner, requestVisibleLaunchSync, waitForMountedRoot]);

  const removeRecentNotebook = useCallback(async (path: string) => {
    if (!primaryWindowOwner) return;
    const recentGeneration = recentNotebooksGenerationRef.current + 1;
    recentNotebooksGenerationRef.current = recentGeneration;
    const notebooks = await removeStoredRecentNotebook(path);
    if (mountedRef.current && recentNotebooksGenerationRef.current === recentGeneration) {
      setRecentNotebooks(notebooks);
    }
  }, [primaryWindowOwner]);

  const performDesktopNotebookSwitch = useCallback(async (path?: string) => {
    if (!primaryWindowOwner || trueMobile) return null;
    let targetPath = path;
    if (targetPath === undefined) {
      const selected = await getAppRuntime().files.openMarkdownFolder();
      if (!selected) return null;
      targetPath = selected.path;
    }

    return runTransaction(async () => {
      let barrierEntered = false;
      let barrierReleased = false;
      const releaseBarrier = async () => {
        if (!barrierEntered || barrierReleased) return;
        barrierReleased = true;
        await appSync.finishNotebookSwitch();
      };
      try {
        await flushActiveDocument();
        barrierEntered = true;
        await appSync.beginNotebookSwitch();
        const root = await primaryWorkspace.commitDesktopRoot(targetPath);
        if (!root) return null;
        return await finishCommittedRoot(root, releaseBarrier);
      } catch {
        return null;
      } finally {
        await releaseBarrier();
      }
    });
  }, [
    appSync,
    finishCommittedRoot,
    flushActiveDocument,
    primaryWindowOwner,
    primaryWorkspace,
    runTransaction,
    trueMobile
  ]);

  const switchManagedNotebook = useCallback(async (name: string) => {
    if (!primaryWindowOwner || !trueMobile || !isValidManagedNotebookName(name)) return null;
    return runTransaction(async () => {
      let barrierEntered = false;
      let barrierReleased = false;
      const releaseBarrier = async () => {
        if (!barrierEntered || barrierReleased) return;
        barrierReleased = true;
        await appSync.finishNotebookSwitch();
      };
      try {
        await flushActiveDocument();
        barrierEntered = true;
        await appSync.beginNotebookSwitch();
        const root = await primaryWorkspace.commitManagedRoot(name);
        if (!root) return null;
        return await finishCommittedRoot(root, releaseBarrier);
      } catch {
        return null;
      } finally {
        await releaseBarrier();
      }
    });
  }, [
    appSync,
    finishCommittedRoot,
    flushActiveDocument,
    primaryWindowOwner,
    primaryWorkspace,
    runTransaction,
    trueMobile
  ]);

  const bootstrapAndCommit = useCallback(async ({
    commit,
    notesRoot,
    preparedTargetLease
  }: {
    commit?: () => Promise<string | null>;
    notesRoot: string;
    preparedTargetLease?: string;
  }) => {
    const revision = configDocument?.revision;
    if (!revision) return null;
    let barrierEntered = false;
    let barrierReleased = false;
    const releaseBarrier = async () => {
      if (!barrierEntered || barrierReleased) return;
      barrierReleased = true;
      await appSync.finishNotebookSwitch();
    };
    try {
      await flushActiveDocument();
      barrierEntered = true;
      await appSync.beginNotebookSwitch();
      const result = await getAppRuntime().syncConfig.sync(preparedTargetLease
        ? {
            bootstrap: true,
            preparedTargetLease,
            revision,
            trigger: "manual"
          }
        : {
            bootstrap: true,
            notesRoot,
            revision,
            trigger: "manual"
          });
      if (!bootstrapResultMatches(result, notesRoot, revision)) return null;
      const root = commit ? await commit() : notesRoot;
      if (!root) return null;
      return await finishCommittedRoot(root, releaseBarrier);
    } catch {
      return null;
    } finally {
      await releaseBarrier();
    }
  }, [appSync, configDocument?.revision, finishCommittedRoot, flushActiveDocument]);

  const restoreDesktopNotebook = useCallback(async (
    remoteName: string,
    parentPath?: string
  ) => {
    if (
      !primaryWindowOwner ||
      trueMobile ||
      !isValidManagedNotebookName(remoteName) ||
      !configDocument?.revision
    ) return null;
    if (desktopRequestInProgressRef.current || pendingRequestRef.current) return null;
    return runTransaction(async () => {
      try {
        let trustedParentPath = parentPath;
        if (trustedParentPath === undefined) {
          const parent = await getAppRuntime().files.openMarkdownFolder();
          if (!parent) return null;
          trustedParentPath = parent.path;
        }
        const prepareTarget = getAppRuntime().workspace.prepareDesktopNotebookTarget;
        if (!prepareTarget) return null;
        const target = await prepareTarget({
          notebookName: remoteName,
          parentPath: trustedParentPath
        });
        if (!target) return null;
        try {
          if (notebookNameFromRoot(target.notesRoot) !== remoteName) return null;
          return await bootstrapAndCommit({
            notesRoot: target.notesRoot,
            preparedTargetLease: target.lease
          });
        } finally {
          await getAppRuntime().workspace
            .discardPreparedDesktopNotebookTarget?.(target.lease)
            .catch(() => undefined);
        }
      } catch {
        return null;
      }
    }, "desktop-remote-restore");
  }, [
    bootstrapAndCommit,
    configDocument?.revision,
    primaryWindowOwner,
    runTransaction,
    trueMobile
  ]);

  const restoreManagedNotebook = useCallback(async (remoteName: string) => {
    if (
      !primaryWindowOwner ||
      !trueMobile ||
      !isValidManagedNotebookName(remoteName) ||
      !configDocument?.revision
    ) return null;
    return runTransaction(async () => {
      try {
        const target = await getAppRuntime().workspace.resolveManagedRoot(remoteName);
        if (!target || notebookNameFromRoot(target) !== remoteName) return null;
        return bootstrapAndCommit({
          commit: () => primaryWorkspace.commitManagedRoot(remoteName),
          notesRoot: target
        });
      } catch {
        return null;
      }
    });
  }, [
    bootstrapAndCommit,
    configDocument?.revision,
    primaryWindowOwner,
    primaryWorkspace,
    runTransaction,
    trueMobile
  ]);

  const performDesktopNotebookSwitchRef = useRef(performDesktopNotebookSwitch);
  performDesktopNotebookSwitchRef.current = performDesktopNotebookSwitch;

  const startDesktopNotebookSwitchPump = useCallback(() => {
    if (requestPumpRef.current) return;
    const pump = (async () => {
      await Promise.resolve();
      while (mountedRef.current) {
        if (activeTransactionRef.current) {
          await activeTransactionRef.current.catch(() => null);
        }
        if (!mountedRef.current) break;
        const pending = pendingRequestRef.current;
        if (!pending) break;
        pendingRequestRef.current = null;
        const { request, settle } = pending;
        let root: string | null = null;
        desktopRequestInProgressRef.current = true;
        try {
          root = request.source === "native-open" && request.path === undefined
            ? null
            : await performDesktopNotebookSwitchRef.current(request.path).catch(() => null);
        } finally {
          desktopRequestInProgressRef.current = false;
        }
        for (const resolve of settle) resolve(root);
      }
    })().finally(() => {
      if (requestPumpRef.current !== pump) return;
      requestPumpRef.current = null;
      if (mountedRef.current && pendingRequestRef.current) {
        startDesktopNotebookSwitchPumpRef.current();
      }
    });
    requestPumpRef.current = pump;
  }, []);
  startDesktopNotebookSwitchPumpRef.current = startDesktopNotebookSwitchPump;

  const requestDesktopNotebookSwitch = useCallback((request: NotebookSwitchRequest) => {
    if (!mountedRef.current || !primaryWindowOwner || trueMobile) return Promise.resolve(null);
    if (activeTransactionKindRef.current === "desktop-remote-restore") {
      return Promise.resolve(null);
    }
    return new Promise<string | null>((resolve) => {
      const pending = pendingRequestRef.current;
      pendingRequestRef.current = {
        request,
        settle: [...(pending?.settle ?? []), resolve]
      };
      startDesktopNotebookSwitchPump();
    });
  }, [primaryWindowOwner, startDesktopNotebookSwitchPump, trueMobile]);

  const switchDesktopNotebook = useCallback((path?: string) => (
    requestDesktopNotebookSwitch({ path, source: "file-menu" })
  ), [requestDesktopNotebookSwitch]);

  useEffect(() => {
    if (!primaryWindowOwner || trueMobile) return;
    let active = true;
    let cleanup: (() => unknown) | null = null;
    listenNotebookSwitchRequested((request) => {
      requestDesktopNotebookSwitch(request).catch(() => null);
    }).then((stopListening) => {
      if (active) cleanup = stopListening;
      else stopListening();
    }).catch(() => {});
    return () => {
      active = false;
      cleanup?.();
    };
  }, [primaryWindowOwner, requestDesktopNotebookSwitch, trueMobile]);

  return {
    recentNotebooks,
    removeRecentNotebook,
    restoreDesktopNotebook,
    restoreManagedNotebook,
    switchDesktopNotebook,
    switchManagedNotebook,
    switching
  };
}
