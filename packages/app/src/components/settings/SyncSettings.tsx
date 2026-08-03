import { AlertTriangle, Cloud, RefreshCw, RotateCcw, TestTube2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  applySyncConfigPatch,
  type DejavuKeyState,
  type DejavuRepositoryStatus,
  type QingYuSyncConfig,
  type SyncConfigDocument,
  type SyncConfigLoadResult,
  type SyncConfigPatch,
  type SyncConfigReadiness,
  type SyncConnectionTestResult,
  type SyncConflictRecord,
  type SyncStatus,
  type SyncTrigger
} from "../../lib/sync-config";
import { getAppRuntime } from "../../runtime";
import { listenDejavuRepositoryBindingChanged } from "../../lib/sync-config-events";
import {
  SettingsButton,
  SettingsCallout,
  SettingsNumberInput,
  SettingsRow,
  SettingsSection,
  SettingsSelect,
  SettingsSwitch,
  SettingsTextInput
} from "./SettingsControls";
import type { SettingsTranslate } from "./translate";

export type SyncSettingsProps = {
  configDocument: SyncConfigDocument | null;
  dejavuSyncAvailable: boolean;
  loadResult: SyncConfigLoadResult | null;
  primaryRoot: string | null;
  saving: boolean;
  status: SyncStatus | null;
  syncRunning: boolean;
  testing: boolean;
  translate: SettingsTranslate;
  onEnable: () => Promise<unknown>;
  onOpenConflictHistory: (conflict: SyncConflictRecord) => unknown;
  onRepositoryIdentityChange?: (identity: {
    notesRoot: string | null;
    repositoryId: string | null;
  }) => unknown;
  onPatch: (patch: SyncConfigPatch) => Promise<unknown>;
  onReset: () => Promise<unknown>;
  onRunSync: () => Promise<unknown>;
  onSelectCloudNotebook: () => Promise<unknown>;
  onTestConnection: () => Promise<SyncConnectionTestResult | undefined>;
};

function configDocumentFromResult(result: unknown): SyncConfigDocument | null {
  if (!result || typeof result !== "object") return null;
  const candidate = result as Partial<SyncConfigDocument>;
  if (
    !candidate.config
    || typeof candidate.configured !== "boolean"
    || !candidate.revision
    || !candidate.readiness
    || !Array.isArray(candidate.issues)
  ) {
    return null;
  }
  return candidate as SyncConfigDocument;
}

type DraftOverlay = {
  operationId: number;
  patch: SyncConfigPatch;
  status: "failed" | "pending";
};

type OptimisticDraftState = {
  awaitingPropRevision: boolean;
  baseConfig: QingYuSyncConfig;
  overlays: Partial<Record<SyncConfigPatch["field"], DraftOverlay>>;
  revision: string;
};

function draftState(config: QingYuSyncConfig, revision: string): OptimisticDraftState {
  return { awaitingPropRevision: false, baseConfig: config, overlays: {}, revision };
}

function overlayValues(overlays: OptimisticDraftState["overlays"]) {
  return Object.values(overlays).filter((overlay): overlay is DraftOverlay => Boolean(overlay));
}

function configWithOverlays(config: QingYuSyncConfig, overlays: OptimisticDraftState["overlays"]) {
  return overlayValues(overlays).reduce(
    (nextConfig, overlay) => applySyncConfigPatch(nextConfig, overlay.patch),
    config
  );
}

function formatStatusDate(value: string | null, translate: SettingsTranslate) {
  if (!value) return translate("settings.sync.never");
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value));
}

function readinessLabel(readiness: SyncConfigReadiness, translate: SettingsTranslate) {
  if (readiness === "ready") return translate("settings.sync.readiness.ready");
  if (readiness === "incomplete") return translate("settings.sync.readiness.incomplete");
  return translate("settings.sync.readiness.disabled");
}

function statusCompletionLabel(status: SyncStatus, translate: SettingsTranslate) {
  if (status.completionState === "attempting") return translate("settings.sync.status.attempting");
  if (status.completionState === "failed") return translate("settings.sync.status.failed");
  return translate("settings.sync.status.succeeded");
}

function statusTriggerLabel(trigger: SyncTrigger, translate: SettingsTranslate) {
  if (trigger === "manual") return translate("settings.sync.trigger.manual");
  if (trigger === "app-launch") return translate("settings.sync.trigger.appLaunch");
  if (trigger === "settings-exit") return translate("settings.sync.trigger.settingsExit");
  if (trigger === "save") return translate("settings.sync.trigger.save");
  return translate("settings.sync.trigger.interval");
}

function dejavuStatusPhaseLabel(status: DejavuRepositoryStatus, translate: SettingsTranslate) {
  if (status.phase === "attempting") return translate("settings.sync.status.attempting");
  if (status.phase === "failed") return translate("settings.sync.status.failed");
  return translate("settings.sync.status.succeeded");
}

function isSafeRelativeStatusPath(value: string) {
  if (
    !value
    || value.startsWith("/")
    || value.includes("\\")
    || /[\u0000-\u001f\u007f-\u009f]/u.test(value)
  ) {
    return false;
  }
  return value.split("/").every((segment) => (
    segment !== ""
    && segment !== "."
    && segment !== ".."
    && !segment.includes(":")
    && !segment.endsWith(".")
    && !segment.endsWith(" ")
  ));
}

function DejavuStatusSummary({
  onOpenConflictHistory,
  status,
  translate
}: {
  onOpenConflictHistory: (conflict: SyncConflictRecord) => unknown;
  status: DejavuRepositoryStatus;
  translate: SettingsTranslate;
}) {
  const conflictHistory = status.conflicts;
  const visibleConflictHistory = conflictHistory.filter(
    (conflict) => isSafeRelativeStatusPath(conflict.relativePath)
  );
  return (
    <div
      aria-label={translate("settings.sync.dejavuStatus.title")}
      className="py-4 text-[12px] leading-5 text-(--text-secondary)"
      role="status"
    >
      <p className="m-0 font-[650] text-(--text-heading)">
        {translate("settings.sync.dejavuStatus.title")}
      </p>
      <div className="mt-1 grid gap-1">
        <p className="m-0 font-[650] text-(--text-heading)">
          {dejavuStatusPhaseLabel(status, translate)}
        </p>
        <p className="m-0">
          {translate("settings.sync.status.trigger")}: {statusTriggerLabel(status.trigger, translate)}
        </p>
        <p className="m-0">
          {translate("settings.sync.status.lastAttempt")}: {formatStatusDate(status.lastAttemptAt, translate)}
        </p>
        <p className="m-0">
          {translate("settings.sync.status.lastSuccess")}: {formatStatusDate(status.lastSuccessfulSyncAt, translate)}
        </p>
        <p className="m-0">
          {translate("settings.sync.status.nextScheduled")}: {formatStatusDate(status.nextScheduledAt, translate)}
        </p>
        {status.error ? (
          <div className="mt-1 rounded-md bg-(--bg-secondary) p-2" aria-label={translate("settings.sync.status.error")}>
            <p className="m-0 font-[650] text-(--text-heading)">{translate("settings.sync.status.error")}</p>
            <p className="m-0">{translate("settings.sync.status.errorCode")}: {status.error.code}</p>
            <p className="m-0">{translate("settings.sync.status.operation")}: {status.error.operation}</p>
          </div>
        ) : null}
        <div className="mt-1 grid gap-0.5" aria-label={translate("settings.sync.status.summary")}>
          <p className="m-0 font-[650] text-(--text-heading)">{translate("settings.sync.status.summary")}</p>
          <p className="m-0">{translate("settings.sync.status.uploadedFiles")}: {status.transfer.uploadFiles}</p>
          <p className="m-0">{translate("settings.sync.status.uploadedChunks")}: {status.transfer.uploadChunks}</p>
          <p className="m-0">{translate("settings.sync.status.bytesUploaded")}: {status.transfer.uploadBytes}</p>
          <p className="m-0">{translate("settings.sync.status.downloadedFiles")}: {status.transfer.downloadFiles}</p>
          <p className="m-0">{translate("settings.sync.status.downloadedChunks")}: {status.transfer.downloadChunks}</p>
          <p className="m-0">{translate("settings.sync.status.bytesDownloaded")}: {status.transfer.downloadBytes}</p>
        </div>
        <p className="m-0">
          {translate("settings.sync.status.lastLocalPurge")}: {formatStatusDate(status.maintenance.lastLocalPurgeAt, translate)}
        </p>
        <p className="m-0">
          {translate("settings.sync.status.nextLocalPurge")}: {formatStatusDate(status.maintenance.nextLocalPurgeAt, translate)}
        </p>
        <p className="m-0">
          {translate("settings.sync.status.conflictHistory")}: {conflictHistory.length}
        </p>
        {visibleConflictHistory.map((conflict) => (
          <button
            className="block w-full break-all py-1 text-left text-[12px] text-(--text-secondary) underline-offset-2 hover:underline"
            key={conflict.conflictId}
            type="button"
            onClick={() => onOpenConflictHistory(conflict)}
          >
            {conflict.relativePath}
          </button>
        ))}
      </div>
    </div>
  );
}

function SyncStatusSummary({ status, translate }: { status: SyncStatus | null; translate: SettingsTranslate }) {
  return (
    <div className="py-4 text-[12px] leading-5 text-(--text-secondary)" role="status" aria-label={translate("settings.sync.lastSync")}>
      <p className="m-0 font-[650] text-(--text-heading)">{translate("settings.sync.lastSync")}</p>
      {!status ? <p className="m-0 mt-1">{translate("settings.sync.status.none")}</p> : (
        <div className="mt-1 grid gap-1">
          <p className="m-0 font-[650] text-(--text-heading)">{statusCompletionLabel(status, translate)}</p>
          <p className="m-0">{translate("settings.sync.status.lastAttempt")}: {formatStatusDate(status.lastAttemptAt, translate)}</p>
          <p className="m-0">{translate("settings.sync.status.lastSuccess")}: {formatStatusDate(status.lastSuccessfulSyncAt, translate)}</p>
          <p className="m-0">{translate("settings.sync.status.provider")}: {status.provider === "s3" ? "S3" : "WebDAV"}</p>
          <p className="m-0">{translate("settings.sync.status.trigger")}: {statusTriggerLabel(status.lastTrigger, translate)}</p>
          {status.error ? (
            <div className="mt-1 rounded-md bg-(--bg-secondary) p-2" aria-label={translate("settings.sync.status.error")}>
              <p className="m-0 font-[650] text-(--text-heading)">{translate("settings.sync.status.error")}</p>
              <p className="m-0">{translate("settings.sync.status.errorCode")}: {status.error.code}</p>
              <p className="m-0">{translate("settings.sync.status.operation")}: {status.error.operation}</p>
              {status.error.httpStatus === null ? null : <p className="m-0">HTTP: {status.error.httpStatus}</p>}
              {status.error.relativePath === null ? null : <p className="m-0">{translate("settings.sync.status.relativePath")}: {status.error.relativePath}</p>}
            </div>
          ) : null}
          {status.summary ? (
            <div className="mt-1 grid gap-0.5" aria-label={translate("settings.sync.status.summary")}>
              <p className="m-0 font-[650] text-(--text-heading)">{translate("settings.sync.status.summary")}</p>
              <p className="m-0">{translate("settings.sync.status.uploadedFiles")}: {status.summary.uploadedFiles}</p>
              <p className="m-0">{translate("settings.sync.status.downloadedFiles")}: {status.summary.downloadedFiles}</p>
              <p className="m-0">{translate("settings.sync.status.conflictFiles")}: {status.summary.conflictFiles}</p>
              <p className="m-0">{translate("settings.sync.status.scannedFiles")}: {status.summary.scannedFiles}</p>
              <p className="m-0">{translate("settings.sync.status.skippedFiles")}: {status.summary.skippedFiles}</p>
              <p className="m-0">{translate("settings.sync.status.bytesUploaded")}: {status.summary.bytesUploaded}</p>
              <p className="m-0">{translate("settings.sync.status.bytesDownloaded")}: {status.summary.bytesDownloaded}</p>
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}

function PrimaryRootRow({
  primaryRoot,
  translate
}: {
  primaryRoot: string | null;
  translate: SettingsTranslate;
}) {
  return (
    <SettingsRow
      title={translate("settings.sync.primaryRoot")}
      description={translate("settings.sync.primaryRootDescription")}
      action={
        <p className="m-0 max-w-80 break-all text-right text-[12px] text-(--text-secondary)">
          {primaryRoot ?? translate("settings.sync.primaryRootMissing")}
        </p>
      }
    />
  );
}

function downloadRepositoryKey(key: string) {
  const blob = new Blob([key], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = window.document.createElement("a");
  try {
    link.href = url;
    link.download = "qingyu-repository-key.txt";
    link.rel = "noopener";
    link.style.display = "none";
    window.document.body.appendChild(link);
    link.click();
  } finally {
    try {
      link.remove();
    } finally {
      URL.revokeObjectURL(url);
    }
  }
}

export function SyncSettings({
  configDocument,
  dejavuSyncAvailable,
  loadResult,
  onEnable,
  onOpenConflictHistory,
  onRepositoryIdentityChange,
  onPatch,
  onReset,
  onRunSync,
  onSelectCloudNotebook,
  onTestConnection,
  primaryRoot,
  saving,
  status,
  syncRunning,
  testing,
  translate
}: SyncSettingsProps) {
  const loadedConfig = configDocument?.config ?? null;
  const loadedRevision = configDocument?.revision ?? null;
  const [draft, setDraft] = useState<OptimisticDraftState | null>(
    loadedConfig && loadedRevision ? draftState(loadedConfig, loadedRevision) : null
  );
  const nextOperationIdRef = useRef(0);
  const bindingAuthorityProbeRef = useRef(0);
  const currentLoadedRevisionRef = useRef(loadedRevision);
  currentLoadedRevisionRef.current = loadedRevision;
  const [connectionTesting, setConnectionTesting] = useState(false);
  const [dejavuKeyState, setDejavuKeyState] = useState<DejavuKeyState | null>(null);
  const [dejavuRepositoryBindingId, setDejavuRepositoryBindingId] = useState<string | null>(null);
  const [dejavuRepositoryStatus, setDejavuRepositoryStatus] = useState<DejavuRepositoryStatus | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [keyFeedback, setKeyFeedback] = useState<string | null>(null);
  const [repositoryFeedback, setRepositoryFeedback] = useState<string | null>(null);
  const [pendingRepositoryOperation, setPendingRepositoryOperation] = useState<string | null>(null);
  const connectionTestingRef = useRef(false);
  const [connectionFeedback, setConnectionFeedback] = useState<{
    result: SyncConnectionTestResult | null;
    state: "failed" | "succeeded";
  } | null>(null);
  const pendingCount = overlayValues(draft?.overlays ?? {}).filter((overlay) => overlay.status === "pending").length;
  const revisionVisible = Boolean(draft && (
    draft.revision === loadedRevision || draft.awaitingPropRevision || pendingCount > 0
  ));
  const visibleOverlays = revisionVisible && draft ? draft.overlays : {};
  const failedOverlays = overlayValues(visibleOverlays).filter((overlay) => overlay.status === "failed");
  const hasUncommittedDraft = overlayValues(visibleOverlays).length > 0;
  const runtime = getAppRuntime();
  const downloadsRepositoryKey = window.isSecureContext === false
    && runtime.features.nativeWindowChrome === false
    && runtime.platform.resolveFormFactor() === "desktop";
  const exportKeyAction = downloadsRepositoryKey
    ? "settings.sync.key.download"
    : "settings.sync.key.export";

  useEffect(() => {
    if (!dejavuSyncAvailable) {
      setDejavuKeyState(null);
      return;
    }
    let active = true;
    getAppRuntime().syncConfig.loadKeyState().then((state) => {
      if (active) setDejavuKeyState(state);
    }).catch(() => {
      if (active) setDejavuKeyState(null);
    });
    return () => {
      active = false;
    };
  }, [dejavuSyncAvailable]);

  useEffect(() => {
    let active = true;
    let cleanup: (() => unknown) | null = null;
    let initialLoadComplete = false;
    let repositoryId: string | null = null;
    let nextOwnershipProbe = 0;
    let adoptedOwnershipEvent = 0;
    const pendingEvents = new Map<string, DejavuRepositoryStatus>();
    setDejavuRepositoryBindingId(null);
    setDejavuRepositoryStatus(null);
    if (!dejavuSyncAvailable || !primaryRoot) {
      return;
    }
    const runtime = getAppRuntime();
    const adoptStatusForCurrentRoot = async (payload: DejavuRepositoryStatus) => {
      const ownershipEvent = ++nextOwnershipProbe;
      const bindingProbe = bindingAuthorityProbeRef.current + 1;
      bindingAuthorityProbeRef.current = bindingProbe;
      try {
        const [binding, current] = await Promise.all([
          runtime.syncConfig.loadRepositoryBinding({ notesRoot: primaryRoot }),
          runtime.syncConfig.loadRepositoryStatus({ notesRoot: primaryRoot })
        ]);
        const currentRepositoryId = binding?.repositoryId ?? current?.repositoryId ?? null;
        if (
          !active
          || currentRepositoryId !== payload.repositoryId
          || ownershipEvent < adoptedOwnershipEvent
          || bindingProbe !== bindingAuthorityProbeRef.current
        ) return;
        adoptedOwnershipEvent = ownershipEvent;
        repositoryId = currentRepositoryId;
        setDejavuRepositoryBindingId(currentRepositoryId);
        setDejavuRepositoryStatus(
          current?.repositoryId === currentRepositoryId ? current : payload
        );
      } catch {
        // An event never bypasses current-root ownership when the binding cannot be reloaded.
      }
    };
    const handleStatus = ({ payload }: { payload: DejavuRepositoryStatus }) => {
      if (!active) return;
      if (!initialLoadComplete) {
        pendingEvents.set(payload.repositoryId, payload);
        return;
      }
      if (repositoryId === payload.repositoryId) setDejavuRepositoryStatus(payload);
      else adoptStatusForCurrentRoot(payload).catch(() => {});
    };
    const completeSupersededInitialization = () => {
      initialLoadComplete = true;
      const pending = [...pendingEvents.values()];
      pendingEvents.clear();
      for (const payload of pending) {
        adoptStatusForCurrentRoot(payload).catch(() => {});
      }
    };
    const initializeStatus = async () => {
      if (runtime.events.isAvailable()) {
        try {
          const stop = await runtime.events.listen<DejavuRepositoryStatus>(
            "qingyu://dejavu-sync-status-changed",
            handleStatus
          );
          if (!active) return stop();
          cleanup = stop;
        } catch {
          // The persisted snapshot remains available when native events are unavailable.
        }
      }
      if (!active) return;
      const bindingProbe = bindingAuthorityProbeRef.current + 1;
      bindingAuthorityProbeRef.current = bindingProbe;
      try {
        const binding = await runtime.syncConfig.loadRepositoryBinding({ notesRoot: primaryRoot });
        let next = await runtime.syncConfig.loadRepositoryStatus({ notesRoot: primaryRoot });
        if (next === null && pendingEvents.size > 0) {
          next = await runtime.syncConfig.loadRepositoryStatus({ notesRoot: primaryRoot });
        }
        if (!active) return;
        if (bindingProbe !== bindingAuthorityProbeRef.current) {
          completeSupersededInitialization();
          return;
        }
        repositoryId = binding?.repositoryId ?? next?.repositoryId ?? null;
        initialLoadComplete = true;
        const pending = repositoryId ? pendingEvents.get(repositoryId) : null;
        pendingEvents.clear();
        setDejavuRepositoryBindingId(repositoryId);
        setDejavuRepositoryStatus(
          pending ?? (next?.repositoryId === repositoryId ? next : null)
        );
      } catch {
        if (!active) return;
        if (bindingProbe !== bindingAuthorityProbeRef.current) {
          completeSupersededInitialization();
          return;
        }
        initialLoadComplete = true;
        pendingEvents.clear();
        setDejavuRepositoryBindingId(null);
        setDejavuRepositoryStatus(null);
      }
    };
    initializeStatus().catch(() => {});
    return () => {
      active = false;
      pendingEvents.clear();
      cleanup?.();
    };
  }, [dejavuSyncAvailable, primaryRoot]);

  useEffect(() => {
    if (!dejavuSyncAvailable || !primaryRoot) return;
    let active = true;
    let cleanup: (() => unknown) | null = null;
    listenDejavuRepositoryBindingChanged(async (payload) => {
      if (!active || payload.notesRoot !== primaryRoot) return;
      const bindingProbe = bindingAuthorityProbeRef.current + 1;
      bindingAuthorityProbeRef.current = bindingProbe;
      try {
        const binding = await getAppRuntime().syncConfig.loadRepositoryBinding({
          notesRoot: primaryRoot
        });
        if (
          !active
          || binding?.repositoryId !== payload.repositoryId
          || bindingProbe !== bindingAuthorityProbeRef.current
        ) return;
        setDejavuRepositoryBindingId(binding.repositoryId);
        setDejavuRepositoryStatus((current) => (
          current?.repositoryId === binding.repositoryId ? current : null
        ));
      } catch {
        // A notification never bypasses the active workspace's persisted binding authority.
      }
    }).then((stop) => {
      if (!active) {
        stop();
        return;
      }
      cleanup = stop;
    }).catch(() => {});
    return () => {
      active = false;
      cleanup?.();
    };
  }, [dejavuSyncAvailable, primaryRoot]);

  useEffect(() => {
    onRepositoryIdentityChange?.({
      notesRoot: primaryRoot,
      repositoryId: dejavuSyncAvailable
        ? dejavuRepositoryBindingId
        : null
    });
  }, [dejavuRepositoryBindingId, dejavuSyncAvailable, onRepositoryIdentityChange, primaryRoot]);

  useEffect(() => {
    setDraft((current) => {
      if (!loadedConfig || !loadedRevision) return null;
      if (!current) return draftState(loadedConfig, loadedRevision);
      if (current.revision === loadedRevision) {
        if (!current.awaitingPropRevision && current.baseConfig === loadedConfig) return current;
        return { ...current, awaitingPropRevision: false, baseConfig: loadedConfig };
      }
      if (overlayValues(current.overlays).some((overlay) => overlay.status === "pending") || current.awaitingPropRevision) {
        return current;
      }
      return draftState(loadedConfig, loadedRevision);
    });
  }, [loadedConfig, loadedRevision, pendingCount]);

  if (!loadResult) {
    return (
      <SettingsSection label={translate("settings.categories.sync")}>
        <PrimaryRootRow primaryRoot={primaryRoot} translate={translate} />
        <SettingsCallout description={translate("settings.sync.loading")} icon={Cloud} title={translate("settings.sync.summaryTitle")} />
      </SettingsSection>
    );
  }

  if (loadResult.status === "absent") {
    return (
      <SettingsSection label={translate("settings.categories.sync")}>
        <PrimaryRootRow primaryRoot={primaryRoot} translate={translate} />
        <SettingsRow
          title={translate("settings.sync.absentTitle")}
          description={translate("settings.sync.absentDescription")}
          action={
            <SettingsButton disabled={saving} label={translate("settings.sync.enableFolder")} onClick={onEnable}>
              <Cloud aria-hidden="true" size={13} />
              {translate("settings.sync.enableFolder")}
            </SettingsButton>
          }
        />
      </SettingsSection>
    );
  }

  if (loadResult.status === "malformed" || loadResult.status === "unsupported") {
    const description = loadResult.status === "unsupported"
      ? `${translate("settings.sync.unsupportedPrefix")} ${loadResult.version} ${translate("settings.sync.unsupportedSuffix")}`
      : translate("settings.sync.malformedDescription");
    const reset = () => {
      const key = loadResult.status === "unsupported"
        ? "settings.sync.unsupportedResetConfirm"
        : "settings.sync.resetConfirm";
      if (window.confirm(translate(key))) return onReset();
    };
    return (
      <SettingsSection label={translate("settings.categories.sync")} intro={
        <SettingsCallout description={description} icon={AlertTriangle} title={translate(loadResult.status === "unsupported" ? "settings.sync.unsupportedTitle" : "settings.sync.malformedTitle")} />
      }>
        <PrimaryRootRow primaryRoot={primaryRoot} translate={translate} />
        <SettingsRow
          title={translate("settings.sync.recoveryTitle")}
          description={translate("settings.sync.recoveryDescription")}
          action={
            <SettingsButton disabled={saving} label={translate("settings.sync.resetConfig")} onClick={reset}>
              <RotateCcw aria-hidden="true" size={13} />
              {translate("settings.sync.resetConfig")}
            </SettingsButton>
          }
        />
      </SettingsSection>
    );
  }

  const baseConfig = revisionVisible && draft?.awaitingPropRevision && draft.revision !== loadedRevision
    ? draft.baseConfig
    : loadResult.config;
  const config = configWithOverlays(baseConfig, visibleOverlays);
  const displayedTesting = testing || connectionTesting;
  const networkActionDisabled = loadResult.readiness !== "ready" || saving || displayedTesting || syncRunning || hasUncommittedDraft;
  const cloudNotebookActionDisabled = !loadResult.configured
    || !primaryRoot
    || saving
    || displayedTesting
    || syncRunning
    || hasUncommittedDraft;
  const queuePatch = (patch: SyncConfigPatch) => {
    const operationId = ++nextOperationIdRef.current;
    setDraft((current) => {
      const next = current ?? draftState(loadResult.config, loadResult.revision);
      return {
        ...next,
        overlays: { ...next.overlays, [patch.field]: { operationId, patch, status: "pending" } }
      };
    });
    return onPatch(patch).then((result) => {
      const document = configDocumentFromResult(result);
      setDraft((current) => {
        if (!current) return current;
        const overlay = current.overlays[patch.field];
        const overlays = { ...current.overlays };
        if (overlay?.operationId === operationId) delete overlays[patch.field];
        if (!document) return { ...current, overlays };
        return {
          awaitingPropRevision: currentLoadedRevisionRef.current !== document.revision,
          baseConfig: document.config,
          overlays,
          revision: document.revision
        };
      });
    }).catch(() => {
      setDraft((current) => {
        if (!current) return current;
        const overlay = current.overlays[patch.field];
        if (overlay?.operationId !== operationId) return current;
        return {
          ...current,
          overlays: { ...current.overlays, [patch.field]: { ...overlay, status: "failed" } }
        };
      });
    });
  };
  const retryFailed = () => failedOverlays.forEach((overlay) => queuePatch(overlay.patch));
  const testConnection = async () => {
    if (connectionTestingRef.current || testing) return;
    connectionTestingRef.current = true;
    setConnectionTesting(true);
    setConnectionFeedback(null);
    try {
      const result = await onTestConnection();
      if (result) setConnectionFeedback({ result, state: "succeeded" });
    } catch {
      setConnectionFeedback({ result: null, state: "failed" });
    } finally {
      connectionTestingRef.current = false;
      setConnectionTesting(false);
    }
  };
  const repositoryId = dejavuRepositoryBindingId;
  const runRepositoryOperation = async (
    operation: string,
    confirmation: string,
    run: () => Promise<unknown>
  ) => {
    if (
      !dejavuSyncAvailable
      || pendingRepositoryOperation === operation
      || !window.confirm(confirmation)
    ) return;
    setPendingRepositoryOperation(operation);
    setRepositoryFeedback(null);
    try {
      await run();
      setRepositoryFeedback(translate("settings.sync.repository.accepted"));
    } catch {
      setRepositoryFeedback(translate("settings.sync.operationFailed"));
    } finally {
      setPendingRepositoryOperation(null);
    }
  };
  const saveGlobalKey = async () => {
    const nextKey = keyInput.trim();
    if (!dejavuSyncAvailable || !nextKey || pendingRepositoryOperation === "key") return;
    setPendingRepositoryOperation("key");
    setKeyFeedback(null);
    try {
      if (dejavuKeyState?.configured) {
        if (!(await getAppRuntime().dialog.confirm(
          translate("settings.sync.key.changeConfirm")
        ))) return;
        await getAppRuntime().syncConfig.changeGlobalKey({ confirmed: true, newKey: nextKey });
      } else {
        setDejavuKeyState(await getAppRuntime().syncConfig.initializeGlobalKey({ key: nextKey }));
      }
      setKeyInput("");
      setKeyFeedback(translate("settings.sync.key.saved"));
    } catch {
      setKeyFeedback(translate("settings.sync.operationFailed"));
    } finally {
      setPendingRepositoryOperation(null);
    }
  };
  const exportGlobalKey = async () => {
    const confirmation = downloadsRepositoryKey
      ? "settings.sync.key.downloadConfirm"
      : "settings.sync.key.exportConfirm";
    if (
      !dejavuSyncAvailable
      || !window.confirm(translate(confirmation))
    ) return;
    setKeyFeedback(null);
    try {
      const key = await getAppRuntime().syncConfig.exportGlobalKey({ confirmed: true });
      if (downloadsRepositoryKey) {
        downloadRepositoryKey(key);
        setKeyFeedback(translate("settings.sync.key.downloaded"));
      } else {
        await navigator.clipboard.writeText(key);
        setKeyFeedback(translate("settings.sync.key.copied"));
      }
    } catch {
      setKeyFeedback(translate("settings.sync.operationFailed"));
    }
  };

  return (
    <>
      <SettingsSection label={translate("settings.sync.section.basic")} intro={!primaryRoot ? (
        <SettingsCallout description={translate("settings.sync.noFolderDescription")} icon={AlertTriangle} title={translate("settings.sync.noFolderTitle")} />
      ) : undefined}>
        <PrimaryRootRow primaryRoot={primaryRoot} translate={translate} />
        <SettingsRow
          title={translate("settings.sync.selectCloudNotebook")}
          description={translate("settings.sync.selectCloudNotebookDescription")}
          action={
            <SettingsButton
              disabled={cloudNotebookActionDisabled}
              label={translate("settings.sync.selectCloudNotebook")}
              onClick={onSelectCloudNotebook}
            >
              <Cloud aria-hidden="true" size={13} />
              {translate("settings.sync.selectCloudNotebook")}
            </SettingsButton>
          }
        />
        <SettingsRow title={translate("settings.sync.enabled")} description={translate("settings.sync.enabledDescription")} action={
          <SettingsSwitch checked={config.enabled} label={translate("settings.sync.enabled")} onChange={() => queuePatch({ field: "enabled", value: !config.enabled })} />
        } />
        <SettingsRow title={translate("settings.sync.provider")} description={translate("settings.sync.providerDescription")} action={
          <SettingsSelect label={translate("settings.sync.provider")} options={[{ label: "WebDAV", value: "webdav" }, { label: "S3", value: "s3" }]} value={config.provider} onChange={(value) => queuePatch({ field: "provider", value: value === "s3" ? "s3" : "webdav" })} />
        } />
        <SettingsRow title={translate("settings.sync.remotePath")} description={translate("settings.sync.remotePathDescription")} action={
          <SettingsTextInput label={translate("settings.sync.remotePath")} value={config.remoteRoot} placeholder={translate("settings.sync.remotePathPlaceholder")} widthClassName="w-64" onChange={(value) => queuePatch({ field: "remoteRoot", value })} />
        } />
      </SettingsSection>
      <SettingsSection label={translate("settings.sync.section.scheduling")}>
        <SettingsRow title={translate("settings.sync.mode")} description={translate("settings.sync.modeDescription")} action={
          <SettingsSelect label={translate("settings.sync.mode")} options={[
            { label: translate("settings.sync.mode.automatic"), value: "automatic" },
            { label: translate("settings.sync.mode.startupExit"), value: "startup-exit" },
            { label: translate("settings.sync.mode.fullyManual"), value: "fully-manual" }
          ]} value={config.mode} onChange={(value) => queuePatch({
            field: "mode",
            value: value === "startup-exit"
              ? "startup-exit"
              : value === "fully-manual"
                ? "fully-manual"
                : "automatic"
          })} />
        } />
        {config.mode === "automatic" ? (
          <SettingsRow title={translate("settings.sync.intervalSeconds")} description={translate("settings.sync.intervalSecondsDescription")} action={
            <SettingsNumberInput label={translate("settings.sync.intervalSeconds")} min={30} max={43200} unit={translate("settings.sync.seconds")} value={config.intervalSeconds} onChange={(value) => queuePatch({ field: "intervalSeconds", value })} />
          } />
        ) : null}
      </SettingsSection>
      {config.provider === "webdav" ? (
        <SettingsSection label={translate("settings.sync.section.webdavConnection")}>
          <SettingsRow title={translate("settings.sync.webdavUrl")} description={translate("settings.sync.webdavUrlDescription")} action={<SettingsTextInput label={translate("settings.sync.webdavUrl")} value={config.webdav.serverUrl} widthClassName="w-64" onChange={(value) => queuePatch({ field: "webdav.serverUrl", value })} />} />
          <SettingsRow title={translate("settings.sync.username")} description={translate("settings.sync.usernameDescription")} action={<SettingsTextInput label={translate("settings.sync.username")} type="password" value={config.webdav.username} onChange={(value) => queuePatch({ field: "webdav.username", value })} />} />
          <SettingsRow title={translate("settings.sync.password")} description={translate("settings.sync.passwordDescription")} action={<SettingsTextInput label={translate("settings.sync.password")} type="password" value={config.webdav.password} onChange={(value) => queuePatch({ field: "webdav.password", value })} />} />
        </SettingsSection>
      ) : (
        <SettingsSection label={translate("settings.sync.section.s3Connection")}>
          <SettingsRow title={translate("settings.sync.s3EndpointUrl")} description={translate("settings.sync.s3EndpointUrlDescription")} action={<SettingsTextInput label={translate("settings.sync.s3EndpointUrl")} value={config.s3.endpointUrl} widthClassName="w-64" onChange={(value) => queuePatch({ field: "s3.endpointUrl", value })} />} />
          <SettingsRow title={translate("settings.sync.s3Region")} description={translate("settings.sync.s3RegionDescription")} action={<SettingsTextInput label={translate("settings.sync.s3Region")} placeholder="auto" value={config.s3.region} onChange={(value) => queuePatch({ field: "s3.region", value })} />} />
          <SettingsRow title={translate("settings.sync.s3Bucket")} description={translate("settings.sync.s3BucketDescription")} action={<SettingsTextInput label={translate("settings.sync.s3Bucket")} value={config.s3.bucket} onChange={(value) => queuePatch({ field: "s3.bucket", value })} />} />
          <SettingsRow title={translate("settings.sync.s3AccessKeyId")} description={translate("settings.sync.s3AccessKeyIdDescription")} action={<SettingsTextInput label={translate("settings.sync.s3AccessKeyId")} type="password" value={config.s3.accessKeyId} onChange={(value) => queuePatch({ field: "s3.accessKeyId", value })} />} />
          <SettingsRow title={translate("settings.sync.s3SecretAccessKey")} description={translate("settings.sync.s3SecretAccessKeyDescription")} action={<SettingsTextInput label={translate("settings.sync.s3SecretAccessKey")} type="password" value={config.s3.secretAccessKey} onChange={(value) => queuePatch({ field: "s3.secretAccessKey", value })} />} />
        </SettingsSection>
      )}
      {config.provider === "s3" && dejavuSyncAvailable ? (
        <>
        <SettingsSection label={translate("settings.sync.section.key")}>
          <SettingsRow
            title={translate("settings.sync.key.title")}
            description={translate(dejavuKeyState?.configured
              ? "settings.sync.key.configured"
              : "settings.sync.key.absent")}
            action={
              <div className="flex flex-wrap justify-end gap-2">
                <SettingsTextInput
                  label={translate("settings.sync.key.input")}
                  placeholder={translate("settings.sync.key.placeholder")}
                  type="password"
                  value={keyInput}
                  onChange={setKeyInput}
                />
                <SettingsButton
                  disabled={!keyInput.trim() || pendingRepositoryOperation === "key"}
                  label={translate(dejavuKeyState?.configured
                    ? "settings.sync.key.change"
                    : "settings.sync.key.import")}
                  onClick={saveGlobalKey}
                >
                  {translate(dejavuKeyState?.configured
                    ? "settings.sync.key.change"
                    : "settings.sync.key.import")}
                </SettingsButton>
                <SettingsButton
                  disabled={!dejavuKeyState?.configured || pendingRepositoryOperation === "key"}
                  label={translate(exportKeyAction)}
                  onClick={exportGlobalKey}
                >
                  {translate(exportKeyAction)}
                </SettingsButton>
              </div>
            }
          />
          {keyFeedback ? <p className="m-0 py-2 text-[12px] text-(--text-secondary)" role="status">{keyFeedback}</p> : null}
        </SettingsSection>
        <SettingsSection label={translate("settings.sync.section.repository") }>
          {!repositoryId ? (
            <p className="m-0 py-4 text-[12px] text-(--text-secondary)">{translate("settings.sync.repository.unbound")}</p>
          ) : (
            <>
              <p className="m-0 py-2 text-[11px] text-(--text-secondary)">{repositoryId}</p>
              {dejavuRepositoryStatus ? (
              <DejavuStatusSummary
                onOpenConflictHistory={onOpenConflictHistory}
                status={dejavuRepositoryStatus}
                translate={translate}
              />
              ) : null}
              <SettingsRow title={translate("settings.sync.repository.rebuild")} description={translate("settings.sync.repository.rebuildDescription")} action={
                <SettingsButton disabled={pendingRepositoryOperation === "rebuild"} label={translate("settings.sync.repository.rebuild")} onClick={() => runRepositoryOperation(
                  "rebuild",
                  translate("settings.sync.repository.rebuildConfirm"),
                  () => getAppRuntime().syncConfig.rebuildLocalRepository({ confirmed: true, repositoryId })
                )}>{translate("settings.sync.repository.rebuild")}</SettingsButton>
              } />
              <SettingsRow title={translate("settings.sync.repository.stop")} description={translate("settings.sync.repository.stopDescription")} action={
                <SettingsButton disabled={pendingRepositoryOperation === "stop"} label={translate("settings.sync.repository.stop")} onClick={() => runRepositoryOperation(
                  "stop",
                  translate("settings.sync.repository.stopConfirm"),
                  () => getAppRuntime().syncConfig.stopRepositorySync({ confirmed: true, repositoryId })
                )}>{translate("settings.sync.repository.stop")}</SettingsButton>
              } />
              <SettingsRow title={translate("settings.sync.repository.purge")} description={translate("settings.sync.repository.purgeDescription")} action={
                <SettingsButton disabled={pendingRepositoryOperation === "purge"} label={translate("settings.sync.repository.purge")} onClick={() => runRepositoryOperation(
                  "purge",
                  translate("settings.sync.repository.purgeConfirm"),
                  () => getAppRuntime().syncConfig.purgeRemoteRepository({ confirmed: true, repositoryId })
                )}>{translate("settings.sync.repository.purge")}</SettingsButton>
              } />
              <SettingsRow title={translate("settings.sync.repository.delete")} description={translate("settings.sync.repository.deleteDescription")} action={
                <SettingsButton disabled={pendingRepositoryOperation === "delete"} label={translate("settings.sync.repository.delete")} onClick={() => runRepositoryOperation(
                  "delete",
                  translate("settings.sync.repository.deleteConfirm"),
                  () => getAppRuntime().syncConfig.deleteRemoteRepository({ confirmed: true, repositoryId })
                )}>{translate("settings.sync.repository.delete")}</SettingsButton>
              } />
              {repositoryFeedback ? <p className="m-0 py-2 text-[12px] text-(--text-secondary)" role="status">{repositoryFeedback}</p> : null}
            </>
          )}
        </SettingsSection>
        </>
      ) : null}
      {config.provider === "s3" ? (
        <SettingsSection label={translate("settings.sync.section.advanced")}>
          <SettingsRow title={translate("settings.sync.generateConflictDocument")} description={translate("settings.sync.generateConflictDocumentDescription")} action={<SettingsSwitch checked={config.generateConflictDocument} label={translate("settings.sync.generateConflictDocument")} onChange={() => queuePatch({ field: "generateConflictDocument", value: !config.generateConflictDocument })} />} />
          <SettingsRow title={translate("settings.sync.s3RequestTimeout")} description={translate("settings.sync.s3RequestTimeoutDescription")} action={<SettingsNumberInput label={translate("settings.sync.s3RequestTimeout")} min={5} max={600} unit={translate("settings.sync.seconds")} value={config.s3.requestTimeoutSeconds} onChange={(value) => queuePatch({ field: "s3.requestTimeoutSeconds", value })} />} />
          <SettingsRow title={translate("settings.sync.s3AddressingStyle")} description={translate("settings.sync.s3AddressingStyleDescription")} action={<SettingsSelect label={translate("settings.sync.s3AddressingStyle")} options={[
            { label: translate("settings.sync.s3AddressingStyle.auto"), value: "auto" },
            { label: translate("settings.sync.s3AddressingStyle.path"), value: "path" },
            { label: translate("settings.sync.s3AddressingStyle.virtualHosted"), value: "virtual-hosted" }
          ]} value={config.s3.addressingStyle} onChange={(value) => queuePatch({
            field: "s3.addressingStyle",
            value: value === "path" ? "path" : value === "virtual-hosted" ? "virtual-hosted" : "auto"
          })} />} />
          <SettingsRow title={translate("settings.sync.s3TlsVerification")} description={translate("settings.sync.s3TlsVerificationDescription")} action={<SettingsSelect label={translate("settings.sync.s3TlsVerification")} options={[
            { label: translate("settings.sync.s3TlsVerification.verify"), value: "verify" },
            { label: translate("settings.sync.s3TlsVerification.skip"), value: "skip" }
          ]} value={config.s3.tlsVerification} onChange={(value) => queuePatch({ field: "s3.tlsVerification", value: value === "skip" ? "skip" : "verify" })} />} />
        </SettingsSection>
      ) : null}
      <SettingsSection label={translate("settings.sync.section.connectionStatus")}>
        <div className="py-4">
          <p className="m-0 text-[13px] leading-5 font-[650] text-(--text-heading)">{translate("settings.sync.readinessTitle")}: {readinessLabel(loadResult.readiness, translate)}</p>
          {loadResult.issues.length > 0 ? (
            <ul className="m-0 mt-1 grid gap-1 pl-5 text-[12px] leading-5 text-(--text-secondary)" aria-label={translate("settings.sync.issues")}>
              {loadResult.issues.map((issue) => <li key={`${issue.field}:${issue.code}`}>{issue.message}</li>)}
            </ul>
          ) : null}
        </div>
        {failedOverlays.length > 0 ? (
          <div className="flex items-center justify-between gap-3 py-2" role="alert">
            <p className="m-0 text-[12px] leading-5 text-(--status-error)">{translate("settings.sync.saveFailed")}</p>
            <SettingsButton disabled={saving || pendingCount > 0} label={translate("settings.sync.retryFailed")} onClick={retryFailed}>
              <RefreshCw aria-hidden="true" size={13} />{translate("settings.sync.retryFailed")}
            </SettingsButton>
          </div>
        ) : null}
        <SettingsRow title={translate("settings.sync.testConnection")} description={translate("settings.sync.testConnectionDescription")} action={
          <SettingsButton disabled={networkActionDisabled} label={translate("settings.sync.testConnection")} onClick={testConnection}>
            <TestTube2 aria-hidden="true" size={13} />{translate(displayedTesting ? "settings.sync.testingConnection" : "settings.sync.testConnection")}
          </SettingsButton>
        } />
        {connectionFeedback ? (
          <p className="m-0 py-2 text-[12px] leading-5 text-(--text-secondary)" role="status">
            {translate(connectionFeedback.state === "succeeded" ? "settings.sync.connectionSucceeded" : "settings.sync.connectionFailed")}
            {connectionFeedback.result ? ` ${connectionFeedback.result.provider === "s3" ? "S3" : "WebDAV"}: ${connectionFeedback.result.checkedTarget}` : ""}
          </p>
        ) : null}
        <SettingsRow title={translate("settings.sync.run")} description={translate("settings.sync.runDescription")} action={
          <SettingsButton disabled={!primaryRoot || networkActionDisabled} label={translate("settings.sync.run")} onClick={onRunSync}>
            <RefreshCw aria-hidden="true" size={13} />{translate(syncRunning ? "settings.sync.running" : "settings.sync.run")}
          </SettingsButton>
        } />
        <SyncStatusSummary status={status} translate={translate} />
      </SettingsSection>
    </>
  );
}
