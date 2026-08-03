import { Cloud, KeyRound, LoaderCircle, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  sanitizeDiagnosticText,
  t,
  type AppLanguage
} from "@markra/shared";
import type {
  DejavuKeyState,
  RemoteNotebookCatalogEntry,
  SyncConfigDocument,
  SyncJobStatus
} from "../../lib/sync-config";
import { remoteNotebookDisabledReasonKey } from "../notebooks/remote-notebook-disabled-reason";
import { getAppRuntime } from "../../runtime";

export type CompactRepositoryAccessProps = {
  configDocument: SyncConfigDocument;
  dirty: boolean;
  language: AppLanguage;
  primaryRoot: string;
  saving: boolean;
};

type AcceptedRecoveryJob = {
  jobId: string;
  repositoryId: string;
  revision: string;
};

type BindState =
  | "accepted"
  | "attempting"
  | "failed"
  | "run-unavailable"
  | "start-failed"
  | "status-error"
  | "submitting"
  | "succeeded"
  | null;
type CatalogState = "error" | "idle" | "loaded" | "loading";

type BindAuthority = {
  allowed: boolean;
  catalogRevision: string | null;
  configRevision: string;
  displayName: string | null;
  notesRoot: string;
  repositoryId: string | null;
  selectedEntryKey: string | null;
};

const targetClass = "min-h-11 min-w-11";
const inputClass = "min-h-11 min-w-0 w-full rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-3 text-base text-(--text-primary)";
const primaryButtonClass = `${targetClass} inline-flex w-full items-center justify-center gap-2 rounded-xl bg-(--accent) px-4 text-sm font-semibold text-white disabled:opacity-50`;
const secondaryButtonClass = `${targetClass} inline-flex w-full items-center justify-center gap-2 rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-4 text-sm font-semibold disabled:opacity-50`;
const maxAutomaticStatusReads = 120;

function catalogEntryKey(entry: RemoteNotebookCatalogEntry) {
  return entry.provider === "s3" ? `s3:${entry.repositoryId}` : `webdav:${entry.name}`;
}

function safeRunCode(value: string | null) {
  if (!value) return null;
  return sanitizeDiagnosticText(value).replace(/\b(?:authorization|bearer|password|secret|token)\b.*$/giu, "[redacted]");
}

function safeBindAdmissionCode(error: unknown) {
  if (typeof error !== "object" || error === null) return null;
  try {
    return Reflect.get(error, "code") === "sync_run_unavailable"
      ? "sync_run_unavailable"
      : null;
  } catch {
    return null;
  }
}

function sameBindAuthority(left: BindAuthority, right: BindAuthority | null) {
  return right !== null &&
    left.allowed === right.allowed &&
    left.catalogRevision === right.catalogRevision &&
    left.configRevision === right.configRevision &&
    left.displayName === right.displayName &&
    left.notesRoot === right.notesRoot &&
    left.repositoryId === right.repositoryId &&
    left.selectedEntryKey === right.selectedEntryKey;
}

function delay(milliseconds: number) {
  return new Promise<undefined>((resolve) => {
    globalThis.setTimeout(() => resolve(undefined), milliseconds);
  });
}

export function CompactRepositoryAccess({
  configDocument,
  dirty,
  language,
  primaryRoot,
  saving
}: CompactRepositoryAccessProps) {
  const runtime = getAppRuntime();
  const available = runtime.features.dejavuSync
    && runtime.kernel.availability === "available"
    && configDocument.config.provider === "s3";
  const revision = configDocument.revision;
  const stableConfig = available && configDocument.configured && !dirty && !saving && Boolean(revision);
  const [keyState, setKeyState] = useState<DejavuKeyState | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [keyFeedback, setKeyFeedback] = useState<string | null>(null);
  const [keyError, setKeyError] = useState(false);
  const [keyLoading, setKeyLoading] = useState(available);
  const [keySaving, setKeySaving] = useState(false);
  const [keyReload, setKeyReload] = useState(0);
  const [catalogReload, setCatalogReload] = useState(0);
  const [catalogState, setCatalogState] = useState<CatalogState>("idle");
  const [catalogRevision, setCatalogRevision] = useState<string | null>(null);
  const [entries, setEntries] = useState<RemoteNotebookCatalogEntry[]>([]);
  const [selectedEntryKey, setSelectedEntryKey] = useState<string | null>(null);
  const [acceptedJob, setAcceptedJob] = useState<AcceptedRecoveryJob | null>(null);
  const [bindState, setBindState] = useState<BindState>(null);
  const [bindErrorCode, setBindErrorCode] = useState<string | null>(null);
  const keyGenerationRef = useRef(0);
  const keyRequestRef = useRef(false);
  const catalogGenerationRef = useRef(0);
  const bindGenerationRef = useRef(0);
  const bindAuthorityRef = useRef<BindAuthority | null>(null);
  const bindRequestRef = useRef(false);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      keyGenerationRef.current += 1;
      catalogGenerationRef.current += 1;
      bindGenerationRef.current += 1;
    };
  }, []);

  useEffect(() => {
    const generation = keyGenerationRef.current + 1;
    keyGenerationRef.current = generation;
    setKeyState(null);
    setKeyError(false);
    setKeyFeedback(null);
    setKeyLoading(available);
    if (!available) return;

    runtime.syncConfig.loadKeyState().then((state) => {
      if (!mountedRef.current || keyGenerationRef.current !== generation) return;
      setKeyState(state);
      setKeyLoading(false);
    }).catch(() => {
      if (!mountedRef.current || keyGenerationRef.current !== generation) return;
      setKeyError(true);
      setKeyLoading(false);
    });
  }, [available, keyReload, runtime.syncConfig]);

  useEffect(() => {
    const generation = catalogGenerationRef.current + 1;
    catalogGenerationRef.current = generation;
    setEntries([]);
    setSelectedEntryKey(null);
    setCatalogRevision(null);
    if (!stableConfig || keyState?.configured !== true) {
      setCatalogState("idle");
      return;
    }

    setCatalogState("loading");
    runtime.syncConfig.listNotebooks({ revision }).then((nextEntries) => {
      if (!mountedRef.current || catalogGenerationRef.current !== generation) return;
      setEntries(nextEntries.filter((entry) => entry.provider === "s3"));
      setCatalogRevision(revision);
      setCatalogState("loaded");
    }).catch(() => {
      if (!mountedRef.current || catalogGenerationRef.current !== generation) return;
      setCatalogState("error");
    });
  }, [catalogReload, keyState?.configured, revision, runtime.syncConfig, stableConfig]);

  const binding = bindState === "accepted"
    || bindState === "attempting"
    || bindState === "submitting";
  const recoveryLocked = acceptedJob !== null || binding;

  const saveKey = useCallback(async () => {
    const nextKey = keyInput.trim();
    if (
      !available || !nextKey || keySaving || recoveryLocked ||
      bindRequestRef.current || keyRequestRef.current
    ) return;
    keyRequestRef.current = true;
    try {
      if (
        keyState?.configured &&
        !(await runtime.dialog.confirm(t(language, "settings.sync.key.changeConfirm")))
      ) return;

      const generation = keyGenerationRef.current + 1;
      keyGenerationRef.current = generation;
      catalogGenerationRef.current += 1;
      bindGenerationRef.current += 1;
      setKeySaving(true);
      setKeyFeedback(null);
      setKeyError(false);
      setEntries([]);
      setSelectedEntryKey(null);
      setCatalogRevision(null);
      setCatalogState("idle");
      setBindState(null);
      setBindErrorCode(null);
      try {
        const nextState = keyState?.configured
          ? await runtime.syncConfig.changeGlobalKey({ confirmed: true, newKey: nextKey }).then(() => ({ configured: true }))
          : await runtime.syncConfig.initializeGlobalKey({ key: nextKey });
        if (!mountedRef.current || keyGenerationRef.current !== generation) return;
        setKeyState(nextState);
        setKeyInput("");
        setKeyFeedback(t(language, "settings.sync.key.saved"));
        setCatalogReload((current) => current + 1);
      } catch {
        if (!mountedRef.current || keyGenerationRef.current !== generation) return;
        setKeyInput("");
        setKeyError(true);
      } finally {
        if (mountedRef.current && keyGenerationRef.current === generation) setKeySaving(false);
      }
    } finally {
      keyRequestRef.current = false;
    }
  }, [available, keyInput, keySaving, keyState?.configured, language, recoveryLocked, runtime.dialog, runtime.syncConfig]);

  const refreshKey = () => {
    setKeyReload((current) => current + 1);
    setCatalogReload((current) => current + 1);
  };
  const refreshCatalog = () => setCatalogReload((current) => current + 1);
  const selectedEntry = selectedEntryKey === null
    ? null
    : entries.find((entry) => catalogEntryKey(entry) === selectedEntryKey) ?? null;
  const canBind = Boolean(
    selectedEntry?.provider === "s3" &&
    selectedEntry.available &&
    catalogRevision === revision &&
    stableConfig &&
    !keyError &&
    !keySaving &&
    acceptedJob === null &&
    bindState !== "succeeded" &&
    !binding
  );
  useLayoutEffect(() => {
    bindAuthorityRef.current = {
      allowed: canBind,
      catalogRevision,
      configRevision: revision,
      displayName: selectedEntry?.provider === "s3" ? selectedEntry.displayName : null,
      notesRoot: primaryRoot,
      repositoryId: selectedEntry?.provider === "s3" ? selectedEntry.repositoryId : null,
      selectedEntryKey
    };
    return () => {
      bindAuthorityRef.current = null;
    };
  }, [
    canBind,
    catalogRevision,
    primaryRoot,
    revision,
    selectedEntry,
    selectedEntryKey
  ]);

  const monitorAcceptedJob = async (job: AcceptedRecoveryJob, generation: number) => {
    for (let readCount = 0; readCount < maxAutomaticStatusReads; readCount += 1) {
      let status: SyncJobStatus;
      try {
        status = await runtime.syncConfig.loadJob({ jobId: job.jobId });
      } catch {
        if (!mountedRef.current || bindGenerationRef.current !== generation) return;
        setBindState("status-error");
        setBindErrorCode(null);
        return;
      }
      if (!mountedRef.current || bindGenerationRef.current !== generation) return;
      if (
        status.jobId !== job.jobId ||
        status.provider !== "s3" ||
        status.revision !== job.revision
      ) {
        setBindState("status-error");
        setBindErrorCode(null);
        return;
      }
      if (status.completionState === "attempting") {
        setBindState("attempting");
        await delay(250);
        continue;
      }
      setAcceptedJob(null);
      if (status.completionState === "succeeded") {
        setBindState("succeeded");
        return;
      }
      setBindErrorCode(safeRunCode(status.error?.code ?? null));
      setBindState("failed");
      return;
    }
    if (!mountedRef.current || bindGenerationRef.current !== generation) return;
    setBindState("status-error");
    setBindErrorCode(null);
  };

  const bind = async () => {
    if (
      !canBind || bindRequestRef.current ||
      selectedEntry?.provider !== "s3" || !catalogRevision
    ) return;
    const authority: BindAuthority = {
      allowed: true,
      catalogRevision,
      configRevision: revision,
      displayName: selectedEntry.displayName,
      notesRoot: primaryRoot,
      repositoryId: selectedEntry.repositoryId,
      selectedEntryKey
    };
    bindRequestRef.current = true;
    try {
      try {
        if (!(await runtime.dialog.confirm(
          t(language, "compact.sync.repository.bindConfirm")
        ))) return;
      } catch {
        return;
      }
      if (
        !mountedRef.current ||
        !sameBindAuthority(authority, bindAuthorityRef.current)
      ) return;

      const generation = bindGenerationRef.current + 1;
      bindGenerationRef.current = generation;
      setBindState("submitting");
      setBindErrorCode(null);
      try {
        const accepted = await runtime.syncConfig.bindRepository({
          displayName: selectedEntry.displayName,
          notesRoot: primaryRoot,
          repositoryId: selectedEntry.repositoryId,
          revision: catalogRevision
        });
        if (!mountedRef.current || bindGenerationRef.current !== generation) return;
        if (
          accepted.notesRoot !== primaryRoot ||
          accepted.repositoryId !== selectedEntry.repositoryId
        ) throw new Error("Repository binding identity changed");
        const job = {
          jobId: accepted.jobId,
          repositoryId: accepted.repositoryId,
          revision: catalogRevision
        };
        setAcceptedJob(job);
        setBindState("accepted");
        await monitorAcceptedJob(job, generation);
      } catch (error) {
        if (!mountedRef.current || bindGenerationRef.current !== generation) return;
        const safeCode = safeBindAdmissionCode(error);
        setBindState(safeCode ? "run-unavailable" : "start-failed");
        setBindErrorCode(safeCode);
      }
    } finally {
      bindRequestRef.current = false;
    }
  };

  const retryAcceptedJob = async () => {
    if (!acceptedJob || bindRequestRef.current) return;
    const generation = bindGenerationRef.current + 1;
    bindGenerationRef.current = generation;
    bindRequestRef.current = true;
    setBindState("attempting");
    setBindErrorCode(null);
    try {
      await monitorAcceptedJob(acceptedJob, generation);
    } finally {
      if (bindGenerationRef.current === generation) bindRequestRef.current = false;
    }
  };

  if (!available) return null;

  return (
    <section
      aria-label={t(language, "compact.sync.repository.title")}
      className="mt-6 grid min-w-0 gap-5 border-t border-(--border-subtle) pt-5"
    >
      <div className="grid min-w-0 gap-2">
        <div className="flex min-w-0 items-center gap-2 text-(--text-heading)">
          <KeyRound aria-hidden="true" size={18} />
          <h2 className="m-0 min-w-0 text-base font-semibold">
            {t(language, "settings.sync.key.title")}
          </h2>
        </div>
        {keyLoading ? (
          <p className="m-0 flex min-h-11 items-center gap-2 text-sm text-(--text-secondary)" role="status">
            <LoaderCircle aria-hidden="true" className="animate-spin motion-reduce:animate-none" size={18} />
            {t(language, "compact.sync.repository.keyLoading")}
          </p>
        ) : keyError ? (
          <div className="grid min-w-0 gap-2">
            <p className="m-0 break-words text-sm text-(--status-error)" role="alert">
              {t(language, "compact.sync.repository.keyError")}
            </p>
            <button className={secondaryButtonClass} disabled={keySaving || recoveryLocked} type="button" onClick={refreshKey}>
              <RefreshCw aria-hidden="true" size={18} />
              {t(language, "notebooks.action.retry")}
            </button>
          </div>
        ) : keyState ? (
          <>
            <p className="m-0 break-words text-sm text-(--text-secondary)">
              {t(language, keyState?.configured
                ? "settings.sync.key.configured"
                : "settings.sync.key.absent")}
            </p>
            <label className="grid min-w-0 gap-1.5 text-sm font-medium">
              <span>{t(language, "settings.sync.key.input")}</span>
              <input
                aria-label={t(language, "settings.sync.key.input")}
                autoCapitalize="none"
                autoComplete="off"
                className={inputClass}
                disabled={keySaving || recoveryLocked}
                placeholder={t(language, "settings.sync.key.placeholder")}
                type="password"
                value={keyInput}
                onChange={(event) => {
                  setKeyInput(event.currentTarget.value);
                  setKeyError(false);
                  setKeyFeedback(null);
                }}
              />
            </label>
            <button
              className={primaryButtonClass}
              disabled={keySaving || recoveryLocked || !keyInput.trim()}
              type="button"
              onClick={() => saveKey().catch(() => {})}
            >
              {t(language, keyState?.configured
                ? "settings.sync.key.change"
                : "settings.sync.key.import")}
            </button>
          </>
        ) : null}
        {keyFeedback ? (
          <p className="m-0 break-words text-sm text-(--status-success)" role="status">{keyFeedback}</p>
        ) : null}
      </div>

      <div className="grid min-w-0 gap-3">
        <div className="flex min-w-0 items-center gap-2 text-(--text-heading)">
          <Cloud aria-hidden="true" size={18} />
          <h2 className="m-0 min-w-0 text-base font-semibold">
            {t(language, "compact.sync.repository.catalogTitle")}
          </h2>
        </div>
        {!stableConfig ? (
          <p className="m-0 break-words text-sm text-(--text-secondary)" role="status">
            {t(language, "compact.sync.repository.configPending")}
          </p>
        ) : keyState?.configured !== true ? (
          <p className="m-0 break-words text-sm text-(--text-secondary)">
            {t(language, "compact.sync.repository.keyRequired")}
          </p>
        ) : catalogState === "loading" ? (
          <p className="m-0 flex min-h-11 items-center gap-2 text-sm text-(--text-secondary)" role="status">
            <LoaderCircle aria-hidden="true" className="animate-spin motion-reduce:animate-none" size={18} />
            {t(language, "notebooks.remote.loading")}
          </p>
        ) : catalogState === "error" ? (
          <div className="grid min-w-0 gap-2">
            <p className="m-0 break-words text-sm text-(--status-error)" role="alert">
              {t(language, "notebooks.remote.refreshError")}
            </p>
            <button className={secondaryButtonClass} type="button" onClick={refreshCatalog}>
              <RefreshCw aria-hidden="true" size={18} />
              {t(language, "notebooks.action.retry")}
            </button>
          </div>
        ) : catalogState === "loaded" && entries.length === 0 ? (
          <p className="m-0 break-words text-sm text-(--text-secondary)">
            {t(language, "notebooks.remote.empty")}
          </p>
        ) : catalogState === "loaded" ? (
          <fieldset className="m-0 grid min-w-0 gap-2 border-0 p-0">
            <legend className="sr-only">{t(language, "notebooks.remote.listLabel")}</legend>
            {entries.map((entry) => {
              const entryKey = catalogEntryKey(entry);
              return (
                <label
                  className="grid min-h-11 min-w-0 grid-cols-[auto_minmax(0,1fr)] items-start gap-x-3 rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-3 py-3 has-[:checked]:border-(--accent) has-[:disabled]:opacity-60"
                  key={entryKey}
                >
                  <input
                    aria-label={entry.name}
                    checked={selectedEntryKey === entryKey}
                    className="mt-1 accent-(--accent)"
                    disabled={recoveryLocked || keySaving || !entry.available}
                    name="compact-remote-notebook"
                    type="radio"
                    value={entryKey}
                    onChange={() => {
                      setSelectedEntryKey(entryKey);
                      setBindState(null);
                      setBindErrorCode(null);
                    }}
                  />
                  <span className="min-w-0 break-words text-sm font-semibold">{entry.name}</span>
                  {!entry.available && entry.disabledReason ? (
                    <span className="col-start-2 min-w-0 break-words text-xs text-(--text-secondary)">
                      {t(language, remoteNotebookDisabledReasonKey(entry.disabledReason))}
                    </span>
                  ) : null}
                </label>
              );
            })}
          </fieldset>
        ) : null}

        {catalogState === "loaded" && entries.length > 0 ? (
          <>
            <p className="m-0 rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-3 py-2 text-xs leading-5 text-(--text-secondary)">
              {t(language, "notebooks.remote.mergeWarning")}
            </p>
            <button
              className={primaryButtonClass}
              disabled={!canBind}
              type="button"
              onClick={() => bind().catch(() => {})}
            >
              {t(language, "compact.sync.repository.bind")}
            </button>
          </>
        ) : null}

        {bindState === "submitting" ? (
          <p className="m-0 flex min-h-11 items-center gap-2 break-words text-sm text-(--text-secondary)" role="status">
            <LoaderCircle aria-hidden="true" className="animate-spin motion-reduce:animate-none" size={18} />
            {t(language, "compact.sync.repository.submitting")}
          </p>
        ) : bindState === "accepted" ? (
          <p className="m-0 break-words text-sm text-(--text-secondary)" role="status">
            {t(language, "compact.sync.repository.accepted")}
          </p>
        ) : bindState === "attempting" ? (
          <p className="m-0 flex min-h-11 items-center gap-2 break-words text-sm text-(--text-secondary)" role="status">
            <LoaderCircle aria-hidden="true" className="animate-spin motion-reduce:animate-none" size={18} />
            {t(language, "compact.sync.repository.attempting")}
          </p>
        ) : bindState === "succeeded" ? (
          <div className="grid min-w-0 gap-1" role="status">
            <p className="m-0 break-words text-sm text-(--status-success)">
              {t(language, "compact.sync.repository.succeeded")}
            </p>
            {!configDocument.config.enabled ? (
              <p className="m-0 break-words text-xs text-(--text-secondary)">
                {t(language, "compact.sync.repository.disabledPreserved")}
              </p>
            ) : null}
          </div>
        ) : bindState === "status-error" ? (
          <div className="grid min-w-0 gap-2" role="alert">
            <p className="m-0 break-words text-sm text-(--status-error)">
              {t(language, "compact.sync.repository.statusUnavailable")}
            </p>
            <button
              className={secondaryButtonClass}
              type="button"
              onClick={() => retryAcceptedJob().catch(() => {})}
            >
              <RefreshCw aria-hidden="true" size={18} />
              {t(language, "compact.sync.repository.checkStatus")}
            </button>
          </div>
        ) : bindState === "failed" || bindState === "run-unavailable" || bindState === "start-failed" ? (
          <div className="grid min-w-0 gap-1" role="alert">
            <p className="m-0 break-words text-sm text-(--status-error)">
              {t(language, bindState === "failed"
                ? "compact.sync.repository.failed"
                : bindState === "run-unavailable"
                  ? "compact.sync.repository.runUnavailable"
                : "compact.sync.repository.startFailed")}
            </p>
            {bindErrorCode ? <p className="m-0 break-all text-xs text-(--text-secondary)">{bindErrorCode}</p> : null}
          </div>
        ) : null}
      </div>
    </section>
  );
}
