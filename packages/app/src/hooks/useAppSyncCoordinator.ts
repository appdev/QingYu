import { type MouseEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { I18nKey } from "@markra/shared";
import { dismissAppToast, showAppToast } from "../lib/app-toast";
import { appLogger } from "../lib/app-logger";
import type {
  SyncConfigDocument,
  SyncConfigLoadResult,
  NormalSyncRunRequest,
  SyncProvider,
  SyncRunResult,
  SyncSafeError,
  SyncStatus,
  SyncTrigger
} from "../lib/sync-config";
import { notebookNameFromRoot } from "../lib/sync-config";
import {
  emitSyncRunCompleted,
  listenSyncApplyRequested,
  listenSyncEditing,
  listenSyncRunRequested,
  listenSyncStatusChanged,
  type SyncApplyRequestedPayload,
  type SyncRunRequestedPayload,
  type SyncStatusChangedPayload
} from "../lib/sync-config-events";
import { runApplicationSync } from "../lib/sync";
import { getAppRuntime } from "../runtime";

export type AppSyncCoordinator = {
  beginNotebookSwitch: () => Promise<void>;
  finishNotebookSwitch: () => unknown;
  notifyDocumentSaved: (documentPath: string) => Promise<unknown>;
  run: (trigger: SyncTrigger, revision?: string) => Promise<SyncRunResult | null>;
  running: boolean;
  status: SyncStatus | null;
};

export type AppSyncCoordinatorInput = {
  configDocument: SyncConfigDocument | null;
  onFilesChanged?: (primaryRoot: string) => Promise<unknown> | unknown;
  primaryRoot: string | null;
  reloadConfig: () => Promise<SyncConfigLoadResult | null>;
  translate: (key: I18nKey) => string;
};

type SharedRunOutcome =
  | { state: "cancelled" }
  | { error: unknown; state: "failed" }
  | { result: SyncRunResult; state: "succeeded" };

type SharedRun = {
  callers: Set<Promise<unknown>>;
  completed: boolean;
  failureNotified: boolean;
  filesChangedNotified: boolean;
  key: string;
  primaryRequest: NormalSyncRunRequest;
  promise: Promise<SharedRunOutcome>;
  recoveryPromise: Promise<SyncSafeError> | null;
  rerunSaveRequested: boolean;
  rerunSaveShouldStart: (() => boolean) | null;
  started: boolean;
  trailingSaveStarted: boolean;
};

type CallerOutcome = { error: SyncSafeError | null; result: SyncRunResult | null };
type EditingSession = { sessionId: string };
type PendingApply = EditingSession & { counter: number; revision: string; token: string };
type SettingsApplyLifecycle = {
  notesRoot: string;
  promise: Promise<unknown>;
};

const pendingRuns = new Map<string, SharedRun>();
const inFlightRuns = new Set<SharedRun>();
const inFlightSettingsApplyLifecycles = new Set<SettingsApplyLifecycle>();
let runTail: Promise<unknown> = Promise.resolve(undefined);
let notebookSwitchBarrierActive = false;

const automaticTriggers = new Set<SyncTrigger>([
  "app-launch",
  "interval",
  "save",
  "settings-exit"
]);
const freshnessErrorCodes = new Set([
  "revision-conflict",
  "sync-config-absent",
  "sync-config-malformed",
  "sync-config-unsupported",
  "sync-disabled",
  "sync-not-ready",
  "sync-result-mismatch"
]);
const safeFallbackErrorCodes = new Set([
  ...freshnessErrorCodes,
  "app-data-unavailable",
  "notes-root-unavailable",
  "remote-http-error",
  "s3-catalog-http-failed",
  "s3-catalog-request-failed",
  "s3-catalog-response-invalid",
  "s3-delete-http-failed",
  "s3-delete-request-failed",
  "s3-download-http-failed",
  "s3-download-request-failed",
  "s3-list-http-failed",
  "s3-list-request-failed",
  "s3-list-response-invalid",
  "s3-metadata-http-failed",
  "s3-metadata-request-failed",
  "s3-object-changed",
  "s3-upload-http-failed",
  "s3-upload-request-failed",
  "s3-upload-verification-failed",
  "sync-apply-mismatch",
  "sync-apply-unavailable",
  "sync-editing-active",
  "sync-failed",
  "sync-identity-changed"
]);

function pendingRunKey(request: NormalSyncRunRequest) {
  return `${request.notesRoot}\u0000${request.notebookName}\u0000${request.revision}\u0000${request.applyToken ?? ""}`;
}

function pendingApplyKey(pending: Pick<PendingApply, "revision" | "sessionId" | "token">) {
  return `${pending.sessionId}\u0000${pending.revision}\u0000${pending.token}`;
}

function acquireSharedRun(request: NormalSyncRunRequest, shouldStart: () => boolean) {
  const key = pendingRunKey(request);
  const existing = pendingRuns.get(key);
  if (existing) {
    if (request.trigger === "save" && existing.started && !existing.completed) {
      if (!existing.trailingSaveStarted) {
        existing.rerunSaveRequested = true;
        existing.rerunSaveShouldStart = shouldStart;
        return existing;
      }
    } else {
      return existing;
    }
  }
  const shared: SharedRun = {
    callers: new Set<Promise<unknown>>(),
    completed: false,
    failureNotified: false,
    filesChangedNotified: false,
    key,
    primaryRequest: request,
    promise: Promise.resolve({ state: "cancelled" } as SharedRunOutcome),
    recoveryPromise: null,
    rerunSaveRequested: false,
    rerunSaveShouldStart: null,
    started: false,
    trailingSaveStarted: false
  };
  const execution = runTail.then(async (): Promise<SharedRunOutcome> => {
    if (!shouldStart()) return { state: "cancelled" };
    shared.started = true;
    try {
      let result = await runApplicationSync(request);
      if (
        result.notesRoot !== request.notesRoot ||
        result.notebookName !== request.notebookName ||
        result.revision !== request.revision
      ) {
        throw new Error("sync-result-mismatch");
      }
      if (shared.rerunSaveRequested) {
        const rerunShouldStart = shared.rerunSaveShouldStart;
        shared.rerunSaveRequested = false;
        shared.rerunSaveShouldStart = null;
        shared.trailingSaveStarted = true;
        const saveRequest = { ...request, trigger: "save" as const };
        if (rerunShouldStart?.()) {
          result = await runApplicationSync(saveRequest);
        }
        if (
          result.notesRoot !== saveRequest.notesRoot ||
          result.notebookName !== saveRequest.notebookName ||
          result.revision !== saveRequest.revision
        ) {
          throw new Error("sync-result-mismatch");
        }
      }
      return { result, state: "succeeded" };
    } catch (error) {
      return { error, state: "failed" };
    }
  });
  shared.promise = execution;
  pendingRuns.set(key, shared);
  inFlightRuns.add(shared);
  runTail = execution.then(() => undefined, () => undefined);
  execution.finally(() => {
    shared.completed = true;
    if (pendingRuns.get(key) === shared) pendingRuns.delete(key);
    if (shared.callers.size === 0) inFlightRuns.delete(shared);
  }).catch(() => {});
  return shared;
}

function isReady(document: SyncConfigDocument | null): document is SyncConfigDocument {
  return Boolean(document?.config.enabled && document.readiness === "ready");
}

function fromLoadResult(result: SyncConfigLoadResult | null): SyncConfigDocument | null {
  if (result?.status !== "loaded") return null;
  return {
    config: result.config,
    configured: result.configured,
    issues: result.issues,
    readiness: result.readiness,
    revision: result.revision
  };
}

function fallbackError(error: unknown, provider: SyncProvider): SyncSafeError {
  const message = error instanceof Error ? error.message : String(error);
  const candidate = /^([a-z][a-z0-9-]{0,63})(?::|$)/u.exec(message)?.[1];
  return {
    category: null,
    code: candidate && safeFallbackErrorCodes.has(candidate) ? candidate : "sync-failed",
    httpStatus: null,
    method: null,
    objectId: null,
    operation: "sync",
    provider,
    providerErrorCode: null,
    relativePath: null,
    requestId: null,
    runId: null
  };
}

export function useAppSyncCoordinator({
  configDocument,
  onFilesChanged,
  primaryRoot,
  reloadConfig,
  translate
}: AppSyncCoordinatorInput): AppSyncCoordinator {
  const [editingSession, setEditingSession] = useState<EditingSession | null>(null);
  const [barrierVersion, setBarrierVersion] = useState(0);
  const [runningCount, setRunningCount] = useState(0);
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [timerVersion, setTimerVersion] = useState(0);
  const blockedRevisionRef = useRef<string | null>(null);
  const claimedApplyKeysRef = useRef(new Set<string>());
  const configRef = useRef<SyncConfigDocument | null>(null);
  const barrierRef = useRef<"checking" | "failed" | "ready">("checking");
  const editingCounterRef = useRef(-1);
  const editingSessionRef = useRef<EditingSession | null>(null);
  const generationRef = useRef(0);
  const launchIdentityRef = useRef<string | null>(null);
  const listenerEventsEnabledRef = useRef(true);
  const listenerRegistrationRef = useRef<Promise<unknown>>(Promise.resolve(undefined));
  const mountedRef = useRef(true);
  const notebookSwitchRootRef = useRef<string | null>(null);
  const onFilesChangedRef = useRef(onFilesChanged);
  const pendingApplyRef = useRef<PendingApply | null>(null);
  const notebookSwitchPendingApplyRef = useRef<PendingApply | null>(null);
  const primaryRootRef = useRef<string | null>(null);
  const reloadRef = useRef(reloadConfig);
  const reclaimPendingApplyRef = useRef<() => Promise<unknown>>(async () => undefined);
  const runDetailedRef = useRef<(
    trigger: SyncTrigger,
    revision?: string,
    applyToken?: string,
    ownedSettingsRoot?: string
  ) => Promise<CallerOutcome>>(async () => ({ error: null, result: null }));
  const runningGenerationRef = useRef(0);
  const statusIdentityRef = useRef<string | null>(null);
  const showSyncFailureToastRef = useRef<() => unknown>(() => undefined);
  const syncToastAttemptRef = useRef(0);
  const translateRef = useRef(translate);

  if (primaryRootRef.current !== primaryRoot) {
    syncToastAttemptRef.current += 1;
    generationRef.current += 1;
    primaryRootRef.current = primaryRoot;
    barrierRef.current = primaryRoot ? "checking" : "ready";
    editingCounterRef.current = -1;
    editingSessionRef.current = null;
    pendingApplyRef.current = null;
    claimedApplyKeysRef.current.clear();
    blockedRevisionRef.current = null;
    configRef.current = null;
    launchIdentityRef.current = null;
    statusIdentityRef.current = null;
  }
  configRef.current = configDocument;
  if (
    configDocument &&
    blockedRevisionRef.current &&
    blockedRevisionRef.current !== configDocument.revision
  ) {
    blockedRevisionRef.current = null;
  }
  reloadRef.current = reloadConfig;
  onFilesChangedRef.current = onFilesChanged;
  translateRef.current = translate;

  const beginNotebookSwitch = useCallback(async () => {
    const oldRoot = primaryRootRef.current;
    notebookSwitchRootRef.current = oldRoot;
    notebookSwitchPendingApplyRef.current = pendingApplyRef.current;
    notebookSwitchBarrierActive = true;
    listenerEventsEnabledRef.current = false;
    generationRef.current += 1;
    runningGenerationRef.current = generationRef.current;
    setRunningCount(0);
    statusIdentityRef.current = null;
    setTimerVersion((current) => current + 1);
    const snapshot = await getAppRuntime().syncConfig.loadEditing().catch(() => null);
    if (
      snapshot?.pendingApply &&
      snapshot.pendingApply.state !== "completed" &&
      (
        !notebookSwitchPendingApplyRef.current ||
        snapshot.pendingApply.counter >= notebookSwitchPendingApplyRef.current.counter
      )
    ) {
      notebookSwitchPendingApplyRef.current = {
        counter: snapshot.pendingApply.counter,
        revision: snapshot.pendingApply.revision,
        sessionId: snapshot.pendingApply.sessionId,
        token: snapshot.pendingApply.token
      };
    }
    if (!oldRoot) return;

    const activeRuns = [...inFlightRuns].filter((shared) => (
      shared.started && shared.primaryRequest.notesRoot === oldRoot
    ));
    const activeSettingsApplies = [...inFlightSettingsApplyLifecycles]
      .filter((lifecycle) => lifecycle.notesRoot === oldRoot)
      .map((lifecycle) => lifecycle.promise);
    await Promise.all([
      ...activeRuns.map(async (shared) => {
        await shared.promise;
        await Promise.all([...shared.callers]);
      }),
      ...activeSettingsApplies
    ]);
  }, []);

  const finishNotebookSwitch = useCallback(async () => {
    const switchRoot = notebookSwitchRootRef.current;
    const rootChanged = switchRoot !== primaryRootRef.current;
    let pending = notebookSwitchPendingApplyRef.current;
    let snapshotCounter: number | null = null;
    if (rootChanged) {
      await listenerRegistrationRef.current.catch(() => undefined);
      const snapshot = await getAppRuntime().syncConfig.loadEditing().catch(() => null);
      if (snapshot) {
        snapshotCounter = snapshot.counter;
        editingCounterRef.current = Math.max(editingCounterRef.current, snapshot.counter);
        const authoritative = snapshot.pendingApply?.state === "completed"
          ? null
          : snapshot.pendingApply
            ? {
                counter: snapshot.pendingApply.counter,
                revision: snapshot.pendingApply.revision,
                sessionId: snapshot.pendingApply.sessionId,
                token: snapshot.pendingApply.token
              }
            : null;
        const observed = notebookSwitchPendingApplyRef.current;
        pending = observed && observed.counter > snapshot.counter
          ? observed
          : authoritative;
      }
    }
    if (rootChanged && pending) {
      try {
        await getAppRuntime().syncConfig.cancelApply({
          revision: pending.revision,
          sessionId: pending.sessionId,
          token: pending.token
        });
      } catch {
        // A newer exact native identity wins; cancellation must never clear it.
      }
      claimedApplyKeysRef.current.add(pendingApplyKey(pending));
      if (pendingApplyRef.current && pendingApplyKey(pendingApplyRef.current) === pendingApplyKey(pending)) {
        pendingApplyRef.current = null;
      }
    }
    if (rootChanged) {
      if (
        !pending &&
        snapshotCounter !== null &&
        pendingApplyRef.current &&
        pendingApplyRef.current.counter <= snapshotCounter
      ) {
        pendingApplyRef.current = null;
      }
      editingSessionRef.current = null;
      setEditingSession(null);
      // Listener bootstrap can observe the old editing state while the switch
      // barrier is active. Let the new root retry app-launch after settlement.
      launchIdentityRef.current = null;
    }
    if (notebookSwitchRootRef.current !== switchRoot) return;
    notebookSwitchRootRef.current = null;
    notebookSwitchPendingApplyRef.current = null;
    notebookSwitchBarrierActive = false;
    listenerEventsEnabledRef.current = true;
    setBarrierVersion((current) => current + 1);
    if (switchRoot === primaryRootRef.current) {
      await reclaimPendingApplyRef.current().catch(() => undefined);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    configRef.current = configDocument;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      configRef.current = null;
      syncToastAttemptRef.current += 1;
      editingSessionRef.current = null;
      pendingApplyRef.current = null;
      claimedApplyKeysRef.current.clear();
    };
  }, []);

  useEffect(() => () => {
    syncToastAttemptRef.current += 1;
    dismissAppToast("app-sync");
  }, [configDocument?.revision, primaryRoot]);

  useEffect(() => {
    setEditingSession(null);
    setRunningCount(0);
    setStatus(null);
    setTimerVersion((current) => current + 1);
    const revision = configDocument?.revision;
    const notebookName = primaryRoot ? notebookNameFromRoot(primaryRoot) : "";
    if (!primaryRoot || !revision) return;
    const generation = generationRef.current;
    getAppRuntime().syncConfig.loadStatus().then((loaded) => {
      if (
        mountedRef.current &&
        generationRef.current === generation &&
        primaryRootRef.current === primaryRoot &&
        loaded?.notebookName === notebookName &&
        loaded?.notesRoot === primaryRoot &&
        loaded.revision === revision
      ) {
        statusIdentityRef.current = `${primaryRoot}\u0000${revision}`;
        setStatus(loaded);
      }
    }).catch(() => {});
  }, [configDocument?.revision, primaryRoot]);

  const installReloaded = useCallback((result: SyncConfigLoadResult | null, generation: number) => {
    if (!mountedRef.current || generationRef.current !== generation) return null;
    const document = fromLoadResult(result);
    configRef.current = document;
    if (!isReady(document)) setTimerVersion((current) => current + 1);
    return document;
  }, []);

  const recoverError = useCallback((shared: SharedRun, provider: SyncProvider, error: unknown) => {
    if (shared.recoveryPromise) return shared.recoveryPromise;
    const fallback = fallbackError(error, provider);
    shared.recoveryPromise = getAppRuntime().syncConfig.loadStatus().then((loaded) => (
      loaded?.completionState === "failed" &&
      loaded.notebookName === shared.primaryRequest.notebookName &&
      loaded.notesRoot === shared.primaryRequest.notesRoot &&
      loaded.revision === shared.primaryRequest.revision &&
      loaded.error?.code === fallback.code
        ? loaded.error
        : fallback
    )).catch(() => fallback);
    return shared.recoveryPromise;
  }, []);

  const showSyncFailureToast = useCallback(() => {
    const failureAttempt = ++syncToastAttemptRef.current;
    const retryAction = {
      label: translateRef.current("settings.sync.toastRetry"),
      onClick: (event: MouseEvent<HTMLButtonElement>) => {
        event.preventDefault();
        if (syncToastAttemptRef.current !== failureAttempt) return;
        const root = primaryRootRef.current;
        const document = configRef.current;
        if (!root || !isReady(document)) {
          syncToastAttemptRef.current += 1;
          runDetailedRef.current("manual").catch((error) => {
            appLogger.error("sync", "Manual synchronization retry failed unexpectedly", {
              error: error instanceof Error ? error.message : String(error)
            });
          });
          return;
        }
        const revision = document.revision;
        const retryAttempt = ++syncToastAttemptRef.current;
        const ownsRetry = () => syncToastAttemptRef.current === retryAttempt;
        const ownsIdentity = () => (
          primaryRootRef.current === root && configRef.current?.revision === revision
        );
        const dismissOwnedRetry = () => {
          if (!ownsRetry()) return;
          syncToastAttemptRef.current += 1;
          dismissAppToast("app-sync");
        };
        const restoreOwnedFailure = () => {
          if (!ownsRetry()) return;
          if (ownsIdentity()) {
            showSyncFailureToastRef.current();
            return;
          }
          dismissOwnedRetry();
        };
        showAppToast({
          action: retryAction,
          id: "app-sync",
          message: translateRef.current("settings.sync.toastRetrying"),
          presentation: "sync-error",
          status: "loading"
        });
        runDetailedRef.current("manual").then(({ error, result }) => {
          if (!ownsRetry()) return;
          if (
            result &&
            ownsIdentity() &&
            result.notesRoot === root &&
            result.revision === revision
          ) {
            dismissOwnedRetry();
            return;
          }
          if (error || !result) restoreOwnedFailure();
        }).catch((error) => {
          if (!ownsRetry()) return;
          appLogger.error("sync", "Manual synchronization retry failed unexpectedly", {
            error: error instanceof Error ? error.message : String(error)
          });
          restoreOwnedFailure();
        });
      }
    };
    showAppToast({
      action: retryAction,
      id: "app-sync",
      message: translateRef.current("settings.sync.toastIncomplete"),
      presentation: "sync-error",
      status: "error"
    });
  }, []);
  showSyncFailureToastRef.current = showSyncFailureToast;

  const runDetailed = useCallback(async (
    trigger: SyncTrigger,
    requestedRevision?: string,
    applyToken?: string,
    ownedSettingsRoot?: string
  ): Promise<CallerOutcome> => {
    const root = primaryRootRef.current;
    const document = configRef.current;
    const revision = requestedRevision ?? document?.revision;
    const generation = generationRef.current;
    const settingsApply = trigger === "settings-exit" && Boolean(applyToken);
    const ownedSettingsApply = settingsApply && ownedSettingsRoot === root;
    if (
      !root ||
      !revision ||
      (!settingsApply && (!isReady(document) || document.revision !== revision))
    ) {
      if (trigger === "manual") {
        showAppToast({
          id: "app-sync",
          message: translateRef.current(!root
            ? "settings.sync.missingSource"
            : document?.config.provider === "s3"
              ? "settings.sync.missingS3"
              : "settings.sync.missingWebDav"),
          status: "error"
        });
      }
      return { error: null, result: null };
    }
    if (
      automaticTriggers.has(trigger) &&
      (
        (notebookSwitchBarrierActive && !ownedSettingsApply) ||
        barrierRef.current !== "ready" ||
        (!settingsApply && blockedRevisionRef.current === revision) ||
        (editingSessionRef.current && trigger !== "settings-exit")
      )
    ) return { error: null, result: null };

    const provider = document?.config.provider ?? "webdav";
    const notebookName = notebookNameFromRoot(root);
    const request: NormalSyncRunRequest = {
      ...(applyToken ? { applyToken } : {}),
      notebookName,
      notesRoot: root,
      revision,
      trigger
    };
    const shared = acquireSharedRun(request, () => (
      (!notebookSwitchBarrierActive || ownedSettingsApply) &&
      (ownedSettingsApply || generationRef.current === generation) &&
      primaryRootRef.current === root &&
      (settingsApply || configRef.current?.revision === revision) &&
      (trigger === "manual" || barrierRef.current === "ready") &&
      !(!settingsApply && blockedRevisionRef.current === revision && automaticTriggers.has(trigger)) &&
      !(editingSessionRef.current && trigger !== "manual" && trigger !== "settings-exit")
    ));
    if (mountedRef.current && generationRef.current === generation) {
      runningGenerationRef.current = generation;
      setRunningCount((current) => current + 1);
    }

    const caller = (async (): Promise<CallerOutcome> => {
      try {
      const outcome = await shared.promise;
      if (outcome.state === "cancelled") return { error: null, result: null };
      if (outcome.state === "failed") {
        const safeError = await recoverError(shared, provider, outcome.error);
        if (mountedRef.current && generationRef.current === generation && primaryRootRef.current === root) {
          if (freshnessErrorCodes.has(safeError.code)) {
            blockedRevisionRef.current = revision;
            setTimerVersion((current) => current + 1);
            const reloaded = await reloadRef.current().catch(() => null);
            installReloaded(reloaded, generation);
          }
          if (!shared.failureNotified) {
            shared.failureNotified = true;
            appLogger.error("sync", "Application synchronization failed", {
              category: safeError.category,
              code: safeError.code,
              httpStatus: safeError.httpStatus,
              method: safeError.method,
              objectId: safeError.objectId,
              operation: safeError.operation,
              provider: safeError.provider,
              providerErrorCode: safeError.providerErrorCode,
              requestId: safeError.requestId,
              runId: safeError.runId
            });
            showSyncFailureToast();
          }
        }
        return { error: safeError, result: null };
      }

      if (mountedRef.current && generationRef.current === generation && primaryRootRef.current === root) {
        if (onFilesChangedRef.current && !shared.filesChangedNotified) {
          shared.filesChangedNotified = true;
          await Promise.resolve(onFilesChangedRef.current(root)).catch(() => {});
        }
      }
      return {
        error: null,
        result: { ...outcome.result, trigger }
      };
      } finally {
        if (mountedRef.current && generationRef.current === generation) {
          runningGenerationRef.current = generation;
          setRunningCount((current) => Math.max(0, current - 1));
        }
      }
    })();
    shared.callers.add(caller);
    caller.finally(() => {
      shared.callers.delete(caller);
      if (shared.completed && shared.callers.size === 0) inFlightRuns.delete(shared);
    }).catch(() => {});
    return caller;
  }, [installReloaded, recoverError, showSyncFailureToast]);
  runDetailedRef.current = runDetailed;

  const run = useCallback(async (trigger: SyncTrigger, revision?: string) => (
    (await runDetailed(trigger, revision)).result
  ), [runDetailed]);

  const notifyDocumentSaved = useCallback(async (documentPath: string) => {
    const root = primaryRootRef.current;
    const document = configRef.current;
    if (!root || !isReady(document) || !document.config.autoSyncOnSave) return null;
    const generation = generationRef.current;
    let member = false;
    try {
      const checkMembership = getAppRuntime().workspace.isDocumentInRoot;
      if (!checkMembership) throw new Error("workspace-document-membership-unavailable");
      member = await checkMembership(documentPath, root);
    } catch {
      if (mountedRef.current && generationRef.current === generation && primaryRootRef.current === root) {
        appLogger.error("sync", "Document sync eligibility check failed", {
          code: "workspace-document-membership-unavailable",
          operation: "sync"
        });
        showSyncFailureToast();
      }
      return null;
    }
    if (
      !member ||
      generationRef.current !== generation ||
      primaryRootRef.current !== root ||
      configRef.current?.revision !== document.revision
    ) return null;
    run("save", document.revision).catch(() => {});
    return true;
  }, [run, showSyncFailureToast]);

  useEffect(() => {
    if (!primaryRoot) return;
    let active = true;
    const cleanups: Array<() => unknown> = [];
    const installed = () => active && mountedRef.current && primaryRootRef.current === primaryRoot;
    const current = () => installed() && listenerEventsEnabledRef.current;
    const stillOwnsRoot = () => active && mountedRef.current && primaryRootRef.current === primaryRoot;
    const rememberPendingApply = (payload: SyncApplyRequestedPayload) => {
      if (payload.state === "completed" || payload.source !== "settings-exit") return false;
      if (editingSessionRef.current && editingSessionRef.current.sessionId !== payload.sessionId) {
        return false;
      }
      if (claimedApplyKeysRef.current.has(pendingApplyKey(payload))) return false;
      const remembered = pendingApplyRef.current;
      if (remembered && payload.counter < remembered.counter) return false;
      if (
        remembered &&
        payload.counter === remembered.counter &&
        pendingApplyKey(payload) !== pendingApplyKey(remembered)
      ) return false;
      pendingApplyRef.current = {
        counter: payload.counter,
        revision: payload.revision,
        sessionId: payload.sessionId,
        token: payload.token
      };
      if (notebookSwitchBarrierActive) {
        notebookSwitchPendingApplyRef.current = pendingApplyRef.current;
      }
      editingCounterRef.current = Math.max(editingCounterRef.current, payload.counter);
      return true;
    };
    const runPending = () => {
      const pending = pendingApplyRef.current;
      if (!current() || barrierRef.current !== "ready" || editingSessionRef.current || !pending) {
        return null;
      }
      const key = pendingApplyKey(pending);
      pendingApplyRef.current = null;
      if (claimedApplyKeysRef.current.has(key)) return null;
      claimedApplyKeysRef.current.add(key);
      let lifecycle!: SettingsApplyLifecycle;
      const promise = Promise.resolve().then(async () => {
        const loaded = await reloadRef.current().catch(() => null);
        if (!stillOwnsRoot()) return;
        const document = fromLoadResult(loaded);
        configRef.current = document;
        if (isReady(document) && document.revision === pending.revision) {
          blockedRevisionRef.current = null;
        }
        if (!document) {
          blockedRevisionRef.current = pending.revision;
          setTimerVersion((value) => value + 1);
        }
        return runDetailedRef.current(
          "settings-exit",
          pending.revision,
          pending.token,
          primaryRoot
        );
      }).finally(() => {
        inFlightSettingsApplyLifecycles.delete(lifecycle);
      });
      lifecycle = { notesRoot: primaryRoot, promise };
      inFlightSettingsApplyLifecycles.add(lifecycle);
      return promise;
    };
    const handleEditing = (payload: { active: boolean; counter: number; revision: string | null; sessionId: string }) => {
      if (
        !stillOwnsRoot() ||
        (!listenerEventsEnabledRef.current && !notebookSwitchBarrierActive) ||
        payload.counter <= editingCounterRef.current
      ) return;
      editingCounterRef.current = payload.counter;
      if (payload.active) {
        const session = { sessionId: payload.sessionId };
        editingSessionRef.current = session;
        setEditingSession(session);
        launchIdentityRef.current = `${primaryRoot}\u0000${configRef.current?.revision ?? ""}`;
        return;
      }
      editingSessionRef.current = null;
      setEditingSession(null);
      if (pendingApplyRef.current && (
        pendingApplyRef.current.sessionId !== payload.sessionId ||
        pendingApplyRef.current.revision !== payload.revision
      )) pendingApplyRef.current = null;
      runPending();
    };
    const handleApply = (payload: SyncApplyRequestedPayload) => {
      if (
        !stillOwnsRoot() ||
        (!listenerEventsEnabledRef.current && !notebookSwitchBarrierActive)
      ) return;
      if (rememberPendingApply(payload)) runPending();
    };
    const handleRequested = (payload: SyncRunRequestedPayload) => {
      if (
        !current() ||
        payload.trigger !== "manual" ||
        payload.notebookName !== notebookNameFromRoot(primaryRoot) ||
        payload.notesRoot !== primaryRoot ||
        editingSessionRef.current?.sessionId !== payload.sessionId
      ) return;
      const execute = async () => {
        const requestGeneration = generationRef.current;
        if (configRef.current?.revision !== payload.revision) {
          installReloaded(await reloadRef.current().catch(() => null), requestGeneration);
        }
        const outcome = await runDetailedRef.current("manual", payload.revision);
        const accepted = Boolean(
          outcome.result?.notebookName === payload.notebookName &&
          outcome.result?.notesRoot === primaryRoot &&
          outcome.result.revision === payload.revision
        );
        await emitSyncRunCompleted({
          accepted,
          error: outcome.error,
          notebookName: payload.notebookName,
          notesRoot: primaryRoot,
          requestId: payload.requestId,
          result: accepted ? outcome.result : null,
          revision: payload.revision,
          sessionId: payload.sessionId,
          trigger: "manual"
        });
      };
      execute().catch(() => {});
    };
    const handleStatus = (payload: SyncStatusChangedPayload) => {
      if (
        !current() ||
        payload.notebookName !== notebookNameFromRoot(primaryRoot) ||
        payload.status.notebookName !== payload.notebookName ||
        payload.notesRoot !== primaryRoot ||
        payload.status.notesRoot !== primaryRoot ||
        payload.revision !== configRef.current?.revision ||
        payload.status.revision !== payload.revision
      ) return;
      statusIdentityRef.current = `${primaryRoot}\u0000${payload.revision}`;
      setStatus(payload.status);
    };
    const registerOne = async (registration: Promise<() => unknown>) => {
      try {
        const cleanup = await registration;
        if (installed()) cleanups.push(cleanup);
        else cleanup();
        return false;
      } catch {
        return true;
      }
    };
    const reclaimPendingApply = async () => {
      const snapshot = await getAppRuntime().syncConfig.loadEditing().catch(() => null);
      if (!current()) return;
      if (snapshot && snapshot.counter >= editingCounterRef.current) {
        editingCounterRef.current = snapshot.counter;
        if (snapshot.pendingApply?.state === "completed") {
          pendingApplyRef.current = null;
        } else if (snapshot.pendingApply) {
          rememberPendingApply(snapshot.pendingApply);
        }
        editingSessionRef.current = snapshot.state
          ? { sessionId: snapshot.state.sessionId }
          : null;
        setEditingSession(editingSessionRef.current);
      }
      await runPending();
    };
    reclaimPendingApplyRef.current = reclaimPendingApply;
    const register = async () => {
      const failures = await Promise.all([
        listenSyncEditing(handleEditing),
        listenSyncApplyRequested(handleApply),
        listenSyncRunRequested(handleRequested),
        listenSyncStatusChanged(handleStatus)
      ].map(registerOne));
      if (!installed()) return;
      if (failures.some(Boolean)) throw new Error("sync-editing-listener-unavailable");
      const snapshot = await getAppRuntime().syncConfig.loadEditing();
      if (!installed()) return;
      if (snapshot.counter >= editingCounterRef.current) {
        editingCounterRef.current = snapshot.counter;
        pendingApplyRef.current = snapshot.pendingApply?.state === "completed"
          ? null
          : snapshot.pendingApply
            ? {
                counter: snapshot.pendingApply.counter,
                revision: snapshot.pendingApply.revision,
                sessionId: snapshot.pendingApply.sessionId,
                token: snapshot.pendingApply.token
              }
            : null;
        editingSessionRef.current = snapshot.state
          ? { sessionId: snapshot.state.sessionId }
          : null;
        setEditingSession(editingSessionRef.current);
        if (editingSessionRef.current || pendingApplyRef.current) {
          launchIdentityRef.current = `${primaryRoot}\u0000${configRef.current?.revision ?? ""}`;
        }
      }
      barrierRef.current = "ready";
      setBarrierVersion((value) => value + 1);
      runPending();
    };
    const registration = register().catch(() => {
      if (!current()) return;
      barrierRef.current = "failed";
      setBarrierVersion((value) => value + 1);
      appLogger.error("sync", "Sync editing state registration failed", {
        code: "sync-editing-state-unavailable",
        operation: "sync"
      });
      showSyncFailureToast();
    });
    let releaseRegistrationWait!: () => void;
    const effectDisposed = new Promise<undefined>((resolve) => {
      releaseRegistrationWait = () => resolve(undefined);
    });
    const registrationWait = Promise.race([registration, effectDisposed]);
    listenerRegistrationRef.current = registrationWait;
    return () => {
      active = false;
      releaseRegistrationWait();
      if (listenerRegistrationRef.current === registrationWait) {
        listenerRegistrationRef.current = Promise.resolve(undefined);
      }
      if (reclaimPendingApplyRef.current === reclaimPendingApply) {
        reclaimPendingApplyRef.current = async () => undefined;
      }
      for (const cleanup of cleanups) cleanup();
    };
  }, [installReloaded, primaryRoot, showSyncFailureToast]);

  useEffect(() => {
    if (!primaryRoot || barrierRef.current !== "ready" || !isReady(configDocument)) return;
    const identity = `${primaryRoot}\u0000${configDocument.revision}`;
    if (launchIdentityRef.current === identity) return;
    launchIdentityRef.current = identity;
    run("app-launch", configDocument.revision).catch(() => {});
  }, [barrierVersion, configDocument, primaryRoot, run]);

  useEffect(() => {
    if (
      !primaryRoot ||
      barrierRef.current !== "ready" ||
      !isReady(configDocument) ||
      configDocument.config.intervalMinutes <= 0 ||
      blockedRevisionRef.current === configDocument.revision ||
      editingSession
    ) return;
    const timer = window.setInterval(() => {
      run("interval", configDocument.revision).catch(() => {});
    }, configDocument.config.intervalMinutes * 60 * 1000);
    return () => window.clearInterval(timer);
  }, [barrierVersion, configDocument, editingSession, primaryRoot, run, timerVersion]);

  const identity = primaryRoot && configDocument
    ? `${primaryRoot}\u0000${configDocument.revision}`
    : null;
  const scopedStatus = statusIdentityRef.current === identity ? status : null;
  const scopedRunning = runningGenerationRef.current === generationRef.current ? runningCount : 0;
  const running = useMemo(
    () => scopedRunning > 0 || scopedStatus?.completionState === "attempting",
    [scopedRunning, scopedStatus?.completionState]
  );
  return {
    beginNotebookSwitch,
    finishNotebookSwitch,
    notifyDocumentSaved,
    run,
    running,
    status: scopedStatus
  };
}
