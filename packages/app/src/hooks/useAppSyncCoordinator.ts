import { type MouseEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { I18nKey } from "@markra/shared";
import { dismissAppToast, showAppToast } from "../lib/app-toast";
import { appLogger } from "../lib/app-logger";
import type {
  DejavuRepositoryStatus,
  SyncConfigDocument,
  SyncConfigLoadResult,
  SyncDispatchResult,
  NormalSyncRunRequest,
  SyncMode,
  SyncProvider,
  SyncRunResult,
  SyncSafeError,
  SyncStatus,
  SyncTrigger
} from "../lib/sync-config";
import { notebookNameFromRoot } from "../lib/sync-config";
import {
  emitSyncRunCompleted,
  listenDejavuSyncStatusChanged,
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
  run: (trigger: SyncTrigger, revision?: string) => Promise<SyncDispatchResult | null>;
  running: boolean;
  status: SyncStatus | null;
};

export type AppSyncCoordinatorInput = {
  configDocument: SyncConfigDocument | null;
  dejavuSyncAvailable: boolean;
  onFilesChanged?: (primaryRoot: string) => Promise<unknown> | unknown;
  primaryRoot: string | null;
  reloadConfig: () => Promise<SyncConfigLoadResult | null>;
  translate: (key: I18nKey) => string;
};

type SharedRunOutcome =
  | { state: "cancelled" }
  | { error: unknown; state: "failed" }
  | { result: SyncDispatchResult; state: "succeeded" };

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

type CallerOutcome = { error: SyncSafeError | null; result: SyncDispatchResult | null };
type EditingSession = { sessionId: string };
type PendingApply = EditingSession & { counter: number; revision: string; token: string };
type SettingsApplyLifecycle = {
  notesRoot: string;
  promise: Promise<unknown>;
};
type AcceptedApplicationRun = {
  generation: number;
  jobId: string;
  notesRoot: string;
  repositoryId: string;
  revision: string;
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
const acceptedStatusPollIntervalMs = 1_000;

function isTriggerEligible(mode: SyncMode, trigger: SyncTrigger) {
  if (trigger === "manual") return true;
  if (mode === "fully-manual") return false;
  if (mode === "startup-exit") {
    return trigger === "app-launch" || trigger === "settings-exit";
  }
  return true;
}
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
  "portable-name-required",
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
const kernelSyncErrorCategories = new Set([
  "authentication",
  "authorization",
  "configuration",
  "conflict",
  "network",
  "provider",
  "storage",
  "transport",
]);
const kernelSyncErrorCodes = new Set([
  "authentication_failed",
  "cancelled",
  "configuration_invalid",
  "conflict",
  "connection_failed",
  "local_io",
  "permission_denied",
  "portable-name-required",
  "rate_limited",
  "remote_unavailable",
  "request_failed",
  "unknown",
]);
const kernelSyncOperations = new Set([
  "apply_config",
  "delete_object",
  "download_object",
  "list_remote",
  "read_local",
  "read_manifest",
  "sync_run",
  "test_connection",
  "upload_object",
  "write_local",
  "write_manifest",
]);
const kernelSyncMethods = new Set(["DELETE", "GET", "HEAD", "POST", "PROPFIND", "PUT"]);
const kernelSyncProviderErrorCodes = new Set([
  "AccessDenied",
  "Conflict",
  "Forbidden",
  "InvalidRequest",
  "Locked",
  "NoSuchBucket",
  "NoSuchKey",
  "NotFound",
  "PreconditionFailed",
  "RequestTimeout",
  "ServerError",
  "SlowDown",
  "TooManyRequests",
  "Unauthorized",
  "Unknown",
]);

function pendingRunKey(request: NormalSyncRunRequest) {
  return `${request.notesRoot}\u0000${request.notebookName}\u0000${request.revision}\u0000${request.applyToken ?? ""}`;
}

function pendingApplyKey(pending: Pick<PendingApply, "revision" | "sessionId" | "token">) {
  return `${pending.sessionId}\u0000${pending.revision}\u0000${pending.token}`;
}

function dispatchMatchesRequest(
  dispatch: SyncDispatchResult,
  request: NormalSyncRunRequest
) {
  if (dispatch.status === "accepted") {
    return dispatch.job.notesRoot === request.notesRoot;
  }
  return dispatch.result.notesRoot === request.notesRoot &&
    dispatch.result.notebookName === request.notebookName &&
    dispatch.result.revision === request.revision;
}

function dispatchWithTrigger(
  dispatch: SyncDispatchResult,
  trigger: SyncTrigger
): SyncDispatchResult {
  if (dispatch.status === "accepted") return dispatch;
  return {
    result: { ...dispatch.result, trigger },
    status: "completed"
  };
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
      if (!dispatchMatchesRequest(result, request)) {
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
        if (!dispatchMatchesRequest(result, saveRequest)) {
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
  const preserved = preservedKernelError(error, provider);
  if (preserved) return preserved;
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

function preservedKernelError(error: unknown, provider: SyncProvider): SyncSafeError | null {
  try {
    if (!isRecord(error)) return null;
    return parseKernelSyncSafeError(ownDataValue(error, "runError"), provider);
  } catch {
    return null;
  }
}

type KernelSyncErrorCategory =
  | "authentication"
  | "authorization"
  | "configuration"
  | "conflict"
  | "network"
  | "provider"
  | "storage"
  | "transport";

export function parseKernelSyncSafeError(value: unknown, provider: SyncProvider): SyncSafeError | null {
  if (!isRecord(value)) return null;
  const keys = [
    "category",
    "code",
    "httpStatus",
    "method",
    "objectId",
    "operation",
    "provider",
    "providerErrorCode",
    "relativePath",
    "requestId",
    "runId",
  ];
  if (
    Reflect.ownKeys(value).length !== keys.length ||
    !keys.every((key) => Object.hasOwn(value, key))
  ) {
    return null;
  }
  const fields = keys.map((key) => ownDataValue(value, key));
  const [
    category,
    code,
    httpStatus,
    method,
    objectId,
    operation,
    errorProvider,
    providerErrorCode,
    relativePath,
    requestId,
    runId,
  ] = fields;
  if (
    (category !== null && !isKernelSyncErrorCategory(category)) ||
    !isKernelSyncErrorCode(code) ||
    !isKernelHttpStatus(httpStatus) ||
    !isNullableKernelMethod(method) ||
    objectId !== null ||
    !isKernelSyncOperation(operation) ||
    errorProvider !== provider ||
    !isNullableKernelProviderErrorCode(providerErrorCode) ||
    !isNullableKernelWorkspaceRelativePath(relativePath) ||
    !isNullableUuid(requestId) ||
    !isNullableUuid(runId)
  ) {
    return null;
  }
  return {
    category,
    code,
    httpStatus,
    method,
    objectId: null,
    operation,
    provider,
    providerErrorCode,
    relativePath,
    requestId,
    runId,
  };
}

function ownDataValue(value: Record<string, unknown>, key: string): unknown {
  const descriptor = Object.getOwnPropertyDescriptor(value, key);
  if (!descriptor || !Object.hasOwn(descriptor, "value")) return undefined;
  return descriptor.value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isKernelSyncErrorCategory(value: unknown): value is KernelSyncErrorCategory {
  return typeof value === "string" && kernelSyncErrorCategories.has(value);
}

function isKernelSyncErrorCode(value: unknown): value is string {
  return typeof value === "string" && kernelSyncErrorCodes.has(value);
}

function isKernelSyncOperation(value: unknown): value is string {
  return typeof value === "string" && kernelSyncOperations.has(value);
}

function isKernelHttpStatus(value: unknown): value is number | null {
  return value === null || (
    typeof value === "number" && Number.isInteger(value) && value >= 100 && value <= 599
  );
}

function isNullableKernelMethod(value: unknown): value is string | null {
  return value === null || (typeof value === "string" && kernelSyncMethods.has(value));
}

function isNullableKernelProviderErrorCode(value: unknown): value is string | null {
  return value === null || (
    typeof value === "string" && kernelSyncProviderErrorCodes.has(value)
  );
}

function isNullableKernelWorkspaceRelativePath(value: unknown): value is string | null {
  return value === null || isKernelWorkspaceRelativePath(value);
}

function isKernelWorkspaceRelativePath(value: unknown): value is string {
  if (typeof value !== "string") return false;
  if (value === "") return true;
  if (
    value.startsWith("/") ||
    value.startsWith("\\") ||
    value.includes("\\") ||
    /[\u0000-\u001f\u007f]/u.test(value) ||
    /^[A-Za-z]:/u.test(value)
  ) {
    return false;
  }
  return value.split("/").every((segment) => segment !== "" && segment !== "." && segment !== "..");
}

function isNullableUuid(value: unknown): value is string | null {
  return value === null || (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu.test(value)
  );
}

export function useAppSyncCoordinator({
  dejavuSyncAvailable,
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
  const [acceptedRunVersion, setAcceptedRunVersion] = useState(0);
  const acceptedRunsRef = useRef(new Map<string, AcceptedApplicationRun>());
  const acceptedStatusPollInFlightRef = useRef<object | null>(null);
  const acceptedStatusPollTimerRef = useRef<number | null>(null);
  const pollAcceptedStatusRef = useRef<() => unknown>(() => undefined);
  const acceptedSharedRunsRef = useRef(new WeakSet<SharedRun>());
  const cachedDejavuStatusesRef = useRef(new Map<string, DejavuRepositoryStatus>());
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
  const cancelAcceptedStatusPoll = useCallback(() => {
    if (acceptedStatusPollTimerRef.current !== null) {
      window.clearTimeout(acceptedStatusPollTimerRef.current);
      acceptedStatusPollTimerRef.current = null;
    }
    acceptedStatusPollInFlightRef.current = null;
  }, []);

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
    cancelAcceptedStatusPoll();
    acceptedRunsRef.current.clear();
    acceptedSharedRunsRef.current = new WeakSet<SharedRun>();
    cachedDejavuStatusesRef.current.clear();
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
    cancelAcceptedStatusPoll();
    acceptedRunsRef.current.clear();
    acceptedSharedRunsRef.current = new WeakSet<SharedRun>();
    cachedDejavuStatusesRef.current.clear();
    setAcceptedRunVersion((current) => current + 1);
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
  }, [cancelAcceptedStatusPoll]);

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
      cancelAcceptedStatusPoll();
      acceptedRunsRef.current.clear();
      configRef.current = null;
      syncToastAttemptRef.current += 1;
      editingSessionRef.current = null;
      pendingApplyRef.current = null;
      claimedApplyKeysRef.current.clear();
    };
  }, [cancelAcceptedStatusPoll]);

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
          const matchesIdentity = result?.status === "accepted"
            ? result.job.notesRoot === root
            : result?.result.notesRoot === root && result.result.revision === revision;
          if (
            matchesIdentity &&
            ownsIdentity() &&
            result
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

  const handleDejavuStatus = useCallback((payload: DejavuRepositoryStatus) => {
    cachedDejavuStatusesRef.current.set(payload.jobId, payload);
    while (cachedDejavuStatusesRef.current.size > 32) {
      const oldest = cachedDejavuStatusesRef.current.keys().next().value;
      if (oldest === undefined) break;
      cachedDejavuStatusesRef.current.delete(oldest);
    }
    const accepted = acceptedRunsRef.current.get(payload.jobId);
    if (
      !accepted ||
      accepted.repositoryId !== payload.repositoryId ||
      payload.phase === "attempting"
    ) return;
    acceptedRunsRef.current.delete(payload.jobId);
    cachedDejavuStatusesRef.current.delete(payload.jobId);
    if (acceptedRunsRef.current.size === 0) cancelAcceptedStatusPoll();
    setAcceptedRunVersion((current) => current + 1);
    if (
      !mountedRef.current ||
      generationRef.current !== accepted.generation ||
      primaryRootRef.current !== accepted.notesRoot ||
      configRef.current?.revision !== accepted.revision
    ) return;
    if (payload.phase === "succeeded") {
      Promise.resolve(onFilesChangedRef.current?.(accepted.notesRoot)).catch(() => {});
      return;
    }
    appLogger.error("sync", "Dejavu synchronization failed", {
      code: payload.error?.code ?? "dejavu-repository-unavailable",
      operation: payload.error?.operation ?? "repository-sync",
      provider: "s3"
    });
    showSyncFailureToast();
  }, [cancelAcceptedStatusPoll, showSyncFailureToast]);

  const scheduleAcceptedStatusPoll = useCallback(() => {
    if (
      acceptedStatusPollTimerRef.current !== null ||
      acceptedStatusPollInFlightRef.current !== null ||
      !mountedRef.current ||
      acceptedRunsRef.current.size === 0
    ) return;
    acceptedStatusPollTimerRef.current = window.setTimeout(() => {
      acceptedStatusPollTimerRef.current = null;
      pollAcceptedStatusRef.current();
    }, acceptedStatusPollIntervalMs);
  }, []);

  const pollAcceptedStatus = useCallback(() => {
    if (
      acceptedStatusPollTimerRef.current !== null ||
      acceptedStatusPollInFlightRef.current !== null ||
      !mountedRef.current ||
      acceptedRunsRef.current.size === 0
    ) return;
    const generation = generationRef.current;
    const notesRoot = primaryRootRef.current;
    if (!notesRoot) return;
    const poll = {};
    acceptedStatusPollInFlightRef.current = poll;
    getAppRuntime().syncConfig.loadRepositoryStatus({ notesRoot }).then((current) => {
      if (
        current &&
        mountedRef.current &&
        generationRef.current === generation &&
        primaryRootRef.current === notesRoot
      ) handleDejavuStatus(current);
    }).catch(() => {}).finally(() => {
      if (acceptedStatusPollInFlightRef.current !== poll) return;
      acceptedStatusPollInFlightRef.current = null;
      if (
        mountedRef.current &&
        generationRef.current === generation &&
        primaryRootRef.current === notesRoot &&
        acceptedRunsRef.current.size > 0
      ) scheduleAcceptedStatusPoll();
    });
  }, [handleDejavuStatus, scheduleAcceptedStatusPoll]);
  pollAcceptedStatusRef.current = pollAcceptedStatus;

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
    if (document && !isTriggerEligible(document.config.mode, trigger)) {
      return { error: null, result: null };
    }
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

      const dispatch = dispatchWithTrigger(outcome.result, trigger);
      if (dispatch.status === "accepted" && dejavuSyncAvailable) {
        if (!acceptedSharedRunsRef.current.has(shared)) {
          acceptedSharedRunsRef.current.add(shared);
          if (
            mountedRef.current &&
            generationRef.current === generation &&
            primaryRootRef.current === root &&
            configRef.current?.revision === revision
          ) {
            for (const [jobId, accepted] of acceptedRunsRef.current) {
              if (
                accepted.generation === generation &&
                accepted.notesRoot === dispatch.job.notesRoot &&
                accepted.repositoryId === dispatch.job.repositoryId
              ) {
                acceptedRunsRef.current.delete(jobId);
                cachedDejavuStatusesRef.current.delete(jobId);
              }
            }
            acceptedRunsRef.current.set(dispatch.job.jobId, {
              generation,
              jobId: dispatch.job.jobId,
              notesRoot: dispatch.job.notesRoot,
              repositoryId: dispatch.job.repositoryId,
              revision
            });
            setAcceptedRunVersion((current) => current + 1);
            const cached = cachedDejavuStatusesRef.current.get(dispatch.job.jobId);
            if (cached) handleDejavuStatus(cached);
            if (acceptedRunsRef.current.has(dispatch.job.jobId)) pollAcceptedStatus();
          }
        }
      } else if (
        mountedRef.current &&
        generationRef.current === generation &&
        primaryRootRef.current === root
      ) {
        if (onFilesChangedRef.current && !shared.filesChangedNotified) {
          shared.filesChangedNotified = true;
          await Promise.resolve(onFilesChangedRef.current(root)).catch(() => {});
        }
      }
      return {
        error: null,
        result: dispatch
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
  }, [dejavuSyncAvailable, handleDejavuStatus, installReloaded, pollAcceptedStatus, recoverError, showSyncFailureToast]);
  runDetailedRef.current = runDetailed;

  const run = useCallback(async (trigger: SyncTrigger, revision?: string) => (
    (await runDetailed(trigger, revision)).result
  ), [runDetailed]);

  const notifyDocumentSaved = useCallback(async (documentPath: string) => {
    const root = primaryRootRef.current;
    const document = configRef.current;
    if (!root || !isReady(document) || !isTriggerEligible(document.config.mode, "save")) {
      return null;
    }
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
        const canRunSettingsApply = isReady(document) &&
          document.revision === pending.revision &&
          isTriggerEligible(document.config.mode, "settings-exit");
        if (!canRunSettingsApply) {
          return getAppRuntime().syncConfig.cancelApply({
            revision: pending.revision,
            sessionId: pending.sessionId,
            token: pending.token
          }).catch((error) => {
            appLogger.error("sync", "Sync settings apply cancellation failed", {
              error: error instanceof Error ? error.message : String(error)
            });
          });
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
          outcome.result?.status === "accepted"
            ? outcome.result.job.notesRoot === primaryRoot
            : outcome.result?.result.notebookName === payload.notebookName &&
              outcome.result.result.notesRoot === primaryRoot &&
              outcome.result.result.revision === payload.revision
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
      const registrations = [
        listenSyncEditing(handleEditing),
        listenSyncApplyRequested(handleApply),
        listenSyncRunRequested(handleRequested),
        listenSyncStatusChanged(handleStatus),
        ...(dejavuSyncAvailable
          ? [listenDejavuSyncStatusChanged(handleDejavuStatus)]
          : [])
      ];
      const failures = await Promise.all(registrations.map(registerOne));
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
  }, [dejavuSyncAvailable, handleDejavuStatus, installReloaded, primaryRoot, showSyncFailureToast]);

  useEffect(() => {
    if (
      !primaryRoot ||
      barrierRef.current !== "ready" ||
      !isReady(configDocument) ||
      !isTriggerEligible(configDocument.config.mode, "app-launch")
    ) return;
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
      !isTriggerEligible(configDocument.config.mode, "interval") ||
      blockedRevisionRef.current === configDocument.revision ||
      editingSession
    ) return;
    const timer = window.setInterval(() => {
      run("interval", configDocument.revision).catch(() => {});
    }, configDocument.config.intervalSeconds * 1000);
    return () => window.clearInterval(timer);
  }, [barrierVersion, configDocument, editingSession, primaryRoot, run, timerVersion]);

  const identity = primaryRoot && configDocument
    ? `${primaryRoot}\u0000${configDocument.revision}`
    : null;
  const scopedStatus = statusIdentityRef.current === identity ? status : null;
  const scopedRunning = runningGenerationRef.current === generationRef.current ? runningCount : 0;
  const running = useMemo(
    () => scopedRunning > 0 ||
      acceptedRunsRef.current.size > 0 ||
      scopedStatus?.completionState === "attempting",
    [acceptedRunVersion, identity, scopedRunning, scopedStatus?.completionState]
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
