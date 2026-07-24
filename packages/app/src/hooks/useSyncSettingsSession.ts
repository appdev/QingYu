import { useCallback, useEffect, useRef, useState } from "react";
import type {
  QingYuSyncConfig,
  SyncConfigDocument,
  SyncConfigLoadResult,
  SyncConfigPatch,
  SyncConnectionTestResult
} from "../lib/sync-config";
import { notebookNameFromRoot } from "../lib/sync-config";
import {
  emitSyncApplyRequested,
  emitSyncEditing,
  emitSyncRunRequested,
  listenSyncRunCompleted
} from "../lib/sync-config-events";
import { getAppRuntime } from "../runtime";

export type SyncSettingsSession = {
  dirty: boolean;
  loadResult: SyncConfigLoadResult | null;
  saving: boolean;
  sessionId: string | null;
  testing: boolean;
  begin: () => Promise<unknown>;
  enable: () => Promise<unknown>;
  end: (source: "catalog-handoff" | "category-leave" | "window-close") => Promise<unknown>;
  patch: (patch: SyncConfigPatch) => Promise<unknown>;
  recover: (config: QingYuSyncConfig) => Promise<unknown>;
  reset: () => Promise<unknown>;
  runImmediate: () => Promise<unknown>;
  testConnection: () => Promise<SyncConnectionTestResult | undefined>;
};

type SessionView = Pick<
  SyncSettingsSession,
  "dirty" | "loadResult" | "saving" | "sessionId" | "testing"
>;

type ActiveSession = {
  active: boolean;
  appliedRevision: string | null;
  dirty: boolean;
  editingReleasePromise: Promise<unknown> | null;
  editingReleased: boolean;
  ending: boolean;
  endPromise: Promise<unknown> | null;
  exitApplyDeliveredRevision: string | null;
  exitApplyToken: { revision: string; token: string } | null;
  failedWrites: Map<SyncConfigWriteKey, unknown>;
  generation: number;
  loadResult: SyncConfigLoadResult;
  manualRequests: Map<string, { notebookName: string; notesRoot: string; revision: string }>;
  pendingWrites: number;
  revision: string | null;
  saving: boolean;
  sessionId: string;
  testing: boolean;
  writeTail: Promise<unknown>;
};

type SyncConfigWriteKey = "enable" | "recover" | "reset" | SyncConfigPatch["field"];

const idleView: SessionView = {
  dirty: false,
  loadResult: null,
  saving: false,
  sessionId: null,
  testing: false
};

let nextIdentifier = 0;

function uniqueIdentifier(prefix: string) {
  const randomIdentifier = globalThis.crypto?.randomUUID?.();
  if (randomIdentifier) return `${prefix}-${randomIdentifier}`;
  nextIdentifier += 1;
  return `${prefix}-${Date.now().toString(36)}-${nextIdentifier.toString(36)}`;
}

function loadedResult(document: SyncConfigDocument): SyncConfigLoadResult {
  return { ...document, status: "loaded" };
}

export function useSyncSettingsSession({
  primaryRoot
}: {
  primaryRoot: string | null;
}): SyncSettingsSession {
  const [view, setView] = useState<SessionView>(idleView);
  const activeSessionRef = useRef<ActiveSession | null>(null);
  const beginRef = useRef<Promise<unknown> | null>(null);
  const generationRef = useRef(0);
  const primaryRootRef = useRef(primaryRoot);
  primaryRootRef.current = primaryRoot;

  const isCurrent = useCallback((session: ActiveSession) => (
    session.active &&
    session.generation === generationRef.current &&
    activeSessionRef.current === session
  ), []);

  const updateView = useCallback((session: ActiveSession) => {
    if (!isCurrent(session)) return;
    setView({
      dirty: session.dirty,
      loadResult: session.loadResult,
      saving: session.saving,
      sessionId: session.sessionId,
      testing: session.testing
    });
  }, [isCurrent]);

  const finishSession = useCallback((session: ActiveSession) => {
    session.active = false;
    session.manualRequests.clear();
    if (activeSessionRef.current !== session) return;
    activeSessionRef.current = null;
    setView({
      dirty: false,
      loadResult: session.loadResult,
      saving: false,
      sessionId: null,
      testing: false
    });
  }, []);

  const releaseEditing = useCallback((session: ActiveSession) => {
    if (session.editingReleased) return Promise.resolve(undefined);
    if (session.editingReleasePromise) return session.editingReleasePromise;
    const promise = emitSyncEditing({
      active: false,
      revision: session.revision,
      sessionId: session.sessionId
    }).then((result) => {
      session.editingReleased = true;
      return result;
    }).finally(() => {
      if (session.editingReleasePromise === promise) session.editingReleasePromise = null;
    });
    session.editingReleasePromise = promise;
    return promise;
  }, []);

  const invalidateSession = useCallback((session: ActiveSession | null) => {
    if (!session?.active) return;
    session.active = false;
    session.ending = true;
    session.manualRequests.clear();
    if (activeSessionRef.current === session) activeSessionRef.current = null;
    releaseEditing(session).catch(() => {});
  }, [releaseEditing]);

  useEffect(() => {
    return () => {
      generationRef.current += 1;
      beginRef.current = null;
      invalidateSession(activeSessionRef.current);
    };
  }, [invalidateSession]);

  useEffect(() => {
    let active = true;
    let cleanup: (() => unknown) | null = null;
    listenSyncRunCompleted((payload) => {
      const session = activeSessionRef.current;
      if (!active || !session || !isCurrent(session) || payload.trigger !== "manual") return;
      if (payload.sessionId !== session.sessionId) return;
      const request = session.manualRequests.get(payload.requestId);
      if (!request) return;
      session.manualRequests.delete(payload.requestId);
      if (
        !payload.accepted ||
        payload.error ||
        !payload.result ||
        payload.revision !== request.revision ||
        payload.result.revision !== request.revision ||
        payload.notebookName !== request.notebookName ||
        payload.result.notebookName !== request.notebookName ||
        payload.notesRoot !== request.notesRoot ||
        payload.result.notesRoot !== request.notesRoot ||
        primaryRootRef.current !== request.notesRoot ||
        session.revision !== request.revision
      ) return;
      session.appliedRevision = request.revision;
      session.dirty = false;
      updateView(session);
    }).then((stop) => {
      if (!active) return stop();
      cleanup = stop;
    }).catch(() => {});
    return () => {
      active = false;
      cleanup?.();
    };
  }, [isCurrent, updateView]);

  const begin = useCallback((): Promise<unknown> => {
    const existing = activeSessionRef.current;
    if (existing && isCurrent(existing) && !existing.ending) {
      return Promise.resolve(existing.loadResult);
    }
    if (beginRef.current) return beginRef.current;
    invalidateSession(existing);
    const generation = generationRef.current;
    const sessionId = uniqueIdentifier("session");
    const promise = (async () => {
      const result = await getAppRuntime().syncConfig.load();
      if (generationRef.current !== generation) return null;
      const session: ActiveSession = {
        active: true,
        appliedRevision: result.revision,
        dirty: false,
        editingReleasePromise: null,
        editingReleased: false,
        ending: false,
        endPromise: null,
        exitApplyDeliveredRevision: null,
        exitApplyToken: null,
        failedWrites: new Map(),
        generation,
        loadResult: result,
        manualRequests: new Map(),
        pendingWrites: 0,
        revision: result.revision,
        saving: false,
        sessionId,
        testing: false,
        writeTail: Promise.resolve(undefined)
      };
      activeSessionRef.current = session;
      updateView(session);
      try {
        await emitSyncEditing({
          active: true,
          revision: session.revision,
          sessionId
        });
      } catch (error) {
        finishSession(session);
        throw error;
      }
      return result;
    })().finally(() => {
      if (beginRef.current === promise) beginRef.current = null;
    });
    beginRef.current = promise;
    return promise;
  }, [finishSession, invalidateSession, isCurrent, updateView]);

  const currentSession = useCallback(() => {
    const session = activeSessionRef.current;
    if (!session || !isCurrent(session) || session.ending) {
      throw new Error("Sync settings session is not active");
    }
    return session;
  }, [isCurrent]);

  const enqueueWrite = useCallback((
    key: SyncConfigWriteKey,
    operation: (session: ActiveSession) => Promise<SyncConfigDocument>
  ) => {
    let session: ActiveSession;
    try {
      session = currentSession();
    } catch (error) {
      return Promise.reject(error);
    }
    session.pendingWrites += 1;
    session.saving = true;
    updateView(session);
    const operationPromise = session.writeTail.then(async () => {
      if (!isCurrent(session)) throw new Error("Sync settings session was invalidated");
      let document: SyncConfigDocument;
      try {
        document = await operation(session);
      } catch (error) {
        if (isCurrent(session)) session.failedWrites.set(key, error);
        throw error;
      }
      if (!isCurrent(session)) return document;
      session.failedWrites.delete(key);
      session.revision = document.revision;
      session.loadResult = loadedResult(document);
      session.dirty = session.revision !== session.appliedRevision;
      updateView(session);
      return document;
    });
    session.writeTail = operationPromise.then(() => undefined, () => undefined);
    operationPromise.finally(() => {
      session.pendingWrites = Math.max(0, session.pendingWrites - 1);
      session.saving = session.pendingWrites > 0;
      updateView(session);
    }).catch(() => {});
    return operationPromise;
  }, [currentSession, isCurrent, updateView]);

  const enable = useCallback(() => enqueueWrite("enable", (session) => (
    getAppRuntime().syncConfig.enable({ expectedRevision: session.revision })
  )), [enqueueWrite]);

  const patch = useCallback((configPatch: SyncConfigPatch) => enqueueWrite(configPatch.field, (session) => {
    if (!session.revision) return Promise.reject(new Error("Sync configuration must be enabled before patching"));
    return getAppRuntime().syncConfig.patch({
      expectedRevision: session.revision,
      patch: configPatch
    });
  }), [enqueueWrite]);

  const recover = useCallback((config: QingYuSyncConfig) => enqueueWrite("recover", (session) => {
    if (session.loadResult.status !== "malformed" && session.loadResult.status !== "unsupported") {
      return Promise.reject(new Error("Sync configuration recovery requires a malformed or unsupported state"));
    }
    return getAppRuntime().syncConfig.recover({
      config,
      expectedRevision: session.loadResult.revision
    });
  }), [enqueueWrite]);

  const reset = useCallback(() => enqueueWrite("reset", (session) => (
    getAppRuntime().syncConfig.reset({
      confirmed: true,
      expectedRevision: session.revision
    })
  )), [enqueueWrite]);

  const testConnection = useCallback(async () => {
    const session = currentSession();
    if (session.testing) return;
    session.testing = true;
    updateView(session);
    try {
      await session.writeTail;
      if (!isCurrent(session) || session.ending || !session.revision) {
        throw new Error("Sync settings session was invalidated");
      }
      return await getAppRuntime().syncConfig.testConnection({ revision: session.revision });
    } finally {
      session.testing = false;
      updateView(session);
    }
  }, [currentSession, isCurrent, updateView]);

  const runImmediate = useCallback(async () => {
    const session = currentSession();
    await session.writeTail;
    const notesRoot = primaryRootRef.current;
    if (!isCurrent(session) || session.ending || !session.revision) {
      throw new Error("Sync settings session was invalidated");
    }
    if (!notesRoot) throw new Error("A primary notes workspace is required for synchronization");
    const requestId = uniqueIdentifier("request");
    const revision = session.revision;
    const notebookName = notebookNameFromRoot(notesRoot);
    session.manualRequests.set(requestId, { notebookName, notesRoot, revision });
    try {
      await emitSyncRunRequested({
        notebookName,
        notesRoot,
        requestId,
        revision,
        sessionId: session.sessionId,
        trigger: "manual"
      });
    } catch (error) {
      session.manualRequests.delete(requestId);
      throw error;
    }
  }, [currentSession, isCurrent]);

  const end = useCallback((source: "catalog-handoff" | "category-leave" | "window-close") => {
    const session = activeSessionRef.current;
    if (!session?.active) return Promise.resolve(undefined);
    if (session.endPromise) return session.endPromise;
    session.ending = true;
    const attempt = (async () => {
      await session.writeTail;
      const failedWrite = session.failedWrites.values().next();
      if (!failedWrite.done) throw failedWrite.value;
      if (
        source !== "catalog-handoff" &&
        isCurrent(session) &&
        session.dirty &&
        session.revision &&
        session.exitApplyDeliveredRevision !== session.revision
      ) {
        const revision = session.revision;
        const token = session.exitApplyToken?.revision === revision
          ? session.exitApplyToken.token
          : uniqueIdentifier("apply");
        session.exitApplyToken = { revision, token };
        await emitSyncApplyRequested({
          exitReason: source,
          revision,
          sessionId: session.sessionId,
          source: "settings-exit",
          token
        });
        session.exitApplyDeliveredRevision = revision;
      }
      await releaseEditing(session);
      finishSession(session);
    })();
    const tracked = attempt.catch((error) => {
      if (session.active) {
        session.ending = false;
        updateView(session);
      }
      throw error;
    }).finally(() => {
      if (session.endPromise === tracked) session.endPromise = null;
    });
    session.endPromise = tracked;
    return tracked;
  }, [finishSession, isCurrent, releaseEditing, updateView]);

  return {
    ...view,
    begin,
    enable,
    end,
    patch,
    recover,
    reset,
    runImmediate,
    testConnection
  };
}
