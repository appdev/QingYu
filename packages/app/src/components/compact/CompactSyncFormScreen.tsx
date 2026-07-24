import { ArrowLeft, Check, TestTube2 } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { t, type AppLanguage } from "@markra/shared";
import type { CompactSyncSettingsController } from "../../hooks/useCompactSyncSettings";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import {
  applySyncConfigPatch,
  type QingYuSyncConfig,
  type SyncConfigPatch,
  type SyncProvider
} from "../../lib/sync-config";

type CompactSyncFormScreenProps = {
  controller: CompactSyncSettingsController;
  exitError?: boolean;
  language: AppLanguage;
  mode: "create" | "edit" | "recover";
  navigation: CompactNavigation;
  registerBeforeExit?: (prepareExit: () => Promise<CompactSyncFormExitPreparation>) => unknown;
};

export type CompactSyncFormExitPreparation = {
  shouldEndSession: boolean;
};

type ConfigWriteKey = "enable" | "recover" | SyncConfigPatch["field"];

const targetClass = "min-h-11 min-w-11";
const inputClass = "min-h-11 min-w-0 max-w-full w-full rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-3 text-base text-(--text-primary)";

function defaultSyncConfig(provider: SyncProvider = "webdav"): QingYuSyncConfig {
  return {
    autoSyncOnSave: false,
    enabled: false,
    intervalMinutes: 0,
    provider,
    remoteRoot: "qingyu",
    s3: {
      accessKeyId: "",
      bucket: "",
      endpointUrl: "",
      region: "",
      secretAccessKey: "",
      requestTimeoutSeconds: 60,
      addressingStyle: "auto",
      tlsVerification: "verify"
    },
    version: 2,
    webdav: {
      password: "",
      serverUrl: "",
      username: ""
    }
  };
}

function initialDraft(controller: CompactSyncSettingsController, mode: CompactSyncFormScreenProps["mode"]) {
  if (mode !== "recover" && controller.loadResult?.status === "loaded") {
    return controller.loadResult.config;
  }
  return defaultSyncConfig();
}

function Field({ children, label }: { children: ReactNode; label: string }) {
  return (
    <label className="grid min-w-0 max-w-full gap-1.5 text-sm font-medium text-(--text-heading)">
      <span className="min-w-0 break-words">{label}</span>
      {children}
    </label>
  );
}

export function CompactSyncFormScreen({
  controller,
  exitError = false,
  language,
  mode,
  navigation,
  registerBeforeExit
}: CompactSyncFormScreenProps) {
  const [draft, setDraft] = useState(() => initialDraft(controller, mode));
  const [operationError, setOperationError] = useState(false);
  const [connectionState, setConnectionState] = useState<"failed" | "idle" | "succeeded">("idle");
  const [beginPending, setBeginPending] = useState(controller.available && controller.sessionId === null);
  const [donePending, setDonePending] = useState(false);
  const [recoveryCompleted, setRecoveryCompleted] = useState(false);
  const [testPending, setTestPending] = useState(false);
  const beginPromiseRef = useRef<Promise<unknown> | null>(null);
  const configWriteTailRef = useRef<Promise<unknown>>(Promise.resolve(undefined));
  const enablePromiseRef = useRef<Promise<unknown> | null>(null);
  const exitRequestedRef = useRef(false);
  const failedWriteKeysRef = useRef(new Set<ConfigWriteKey>());
  const recoveryCompletedRef = useRef(false);
  const testPendingRef = useRef(false);
  const mountedRef = useRef(true);
  const latestDraftRef = useRef(draft);
  latestDraftRef.current = draft;

  const reportOperationFailure = useCallback(() => {
    if (mountedRef.current) setOperationError(true);
  }, []);
  const trackConfigWrite = useCallback(<T,>(key: ConfigWriteKey, operation: Promise<T>) => {
    const tracked = operation.then((result) => {
      failedWriteKeysRef.current.delete(key);
      return result;
    }, (error: unknown) => {
      failedWriteKeysRef.current.add(key);
      reportOperationFailure();
      throw error;
    });
    const settled = tracked.then(() => undefined, () => undefined);
    configWriteTailRef.current = Promise.all([
      configWriteTailRef.current,
      settled
    ]).then(() => undefined);
    return tracked;
  }, [reportOperationFailure]);
  const ensureEnabled = useCallback(() => {
    if (mode !== "create" || enablePromiseRef.current) return enablePromiseRef.current;
    const enablePromise = trackConfigWrite("enable", controller.enable());
    enablePromiseRef.current = enablePromise;
    enablePromise.then(undefined, () => {
      if (enablePromiseRef.current === enablePromise) enablePromiseRef.current = null;
    });
    return enablePromise;
  }, [controller.enable, mode, trackConfigWrite]);
  const prepareExit = useCallback(async () => {
    exitRequestedRef.current = true;
    let shouldEndSession = true;
    if (beginPromiseRef.current) {
      try {
        await beginPromiseRef.current;
      } catch {
        shouldEndSession = false;
      }
    }

    while (true) {
      const pendingWrites = configWriteTailRef.current;
      await pendingWrites;
      if (pendingWrites === configWriteTailRef.current) break;
    }

    if (failedWriteKeysRef.current.size > 0) {
      throw new Error("compact-sync-config-write-failed");
    }
    return { shouldEndSession };
  }, []);

  useLayoutEffect(() => {
    registerBeforeExit?.(prepareExit);
  }, [prepareExit, registerBeforeExit]);

  useEffect(() => {
    mountedRef.current = true;
    if (!controller.available) {
      setBeginPending(false);
      return () => {
        mountedRef.current = false;
      };
    }

    exitRequestedRef.current = false;
    const beginWasPending = controller.sessionId === null;
    const begin = controller.begin();
    beginPromiseRef.current = begin;
    begin.then(() => {
      if (mountedRef.current && beginWasPending) setBeginPending(false);
      if (mode === "create" && !exitRequestedRef.current) ensureEnabled();
    }, () => {
      if (!mountedRef.current) return;
      setBeginPending(false);
      setOperationError(true);
    });

    return () => {
      exitRequestedRef.current = true;
      mountedRef.current = false;
    };
  }, [controller.available, controller.begin, ensureEnabled, mode]);

  const update = (patch: SyncConfigPatch) => {
    exitRequestedRef.current = false;
    setDraft((current) => applySyncConfigPatch(current, patch));
    setConnectionState("idle");
    setOperationError(false);
    if (mode === "recover") return;

    if (mode === "create") ensureEnabled();
    trackConfigWrite(patch.field, controller.patch(patch)).then(undefined, () => undefined);
  };
  const back = () => navigation.pop().catch(reportOperationFailure);
  const done = async () => {
    if (donePending) return;
    setDonePending(true);
    setOperationError(false);
    try {
      if (mode === "recover" && !recoveryCompletedRef.current) {
        await trackConfigWrite("recover", controller.recover(latestDraftRef.current));
        if (!mountedRef.current) return;
        recoveryCompletedRef.current = true;
        setRecoveryCompleted(true);
      }
      await navigation.pop();
    } catch {
      setOperationError(true);
    } finally {
      if (mountedRef.current) setDonePending(false);
    }
  };
  const testConnection = async () => {
    if (controller.testing || testPendingRef.current) return;
    testPendingRef.current = true;
    setTestPending(true);
    setConnectionState("idle");
    setOperationError(false);
    try {
      const result = await controller.testConnection();
      if (result && mountedRef.current) setConnectionState("succeeded");
    } catch {
      if (mountedRef.current) setConnectionState("failed");
    } finally {
      testPendingRef.current = false;
      if (mountedRef.current) setTestPending(false);
    }
  };

  if (!controller.available) {
    return (
      <section
        aria-label={t(language, "compact.sync.formTitle")}
        className="absolute inset-0 flex h-full min-h-0 min-w-0 flex-col overflow-x-hidden bg-(--bg-primary)"
        data-testid="compact-sync-form"
      >
        <header className="flex shrink-0 items-center gap-2 border-b border-(--border-subtle) px-2 pt-[var(--compact-safe-area-top)]">
          <button
            aria-label={t(language, "compact.navigation.back")}
            className={`${targetClass} inline-flex items-center justify-center rounded-lg`}
            type="button"
            onClick={back}
          >
            <ArrowLeft aria-hidden="true" size={20} />
          </button>
          <h1 className="m-0 min-w-0 flex-1 truncate text-base font-semibold">
            {t(language, "compact.sync.formTitle")}
          </h1>
        </header>
        <div className="grid min-h-0 min-w-0 flex-1 place-content-center overflow-x-hidden px-4 pb-[var(--compact-bottom-inset)] text-center">
          <h2 className="m-0 text-lg font-semibold">{t(language, "compact.sync.unavailableTitle")}</h2>
          <p className="m-0 mt-2 break-words text-sm text-(--text-secondary)">
            {t(language, "compact.sync.unavailableDescription")}
          </p>
        </div>
      </section>
    );
  }

  const formDisabled = beginPending || donePending || controller.saving;
  const fieldsDisabled = formDisabled || recoveryCompleted || testPending;
  const canTest = mode !== "recover" && controller.loadResult?.status === "loaded"
    && controller.loadResult.readiness === "ready" && !formDisabled && !controller.testing && !testPending;
  const errorVisible = operationError || exitError;

  return (
    <section
      aria-label={t(language, "compact.sync.formTitle")}
      className="absolute inset-0 flex h-full min-h-0 min-w-0 flex-col overflow-x-hidden bg-(--bg-primary)"
      data-testid="compact-sync-form"
    >
      <header className="flex shrink-0 items-center gap-2 border-b border-(--border-subtle) px-2 pt-[var(--compact-safe-area-top)]">
        <button
          aria-label={t(language, "compact.navigation.back")}
          className={`${targetClass} inline-flex items-center justify-center rounded-lg`}
          disabled={donePending}
          type="button"
          onClick={back}
        >
          <ArrowLeft aria-hidden="true" size={20} />
        </button>
        <h1 className="m-0 min-w-0 flex-1 truncate text-base font-semibold">
          {t(language, "compact.sync.formTitle")}
        </h1>
        <button
          className={`${targetClass} rounded-lg px-3 text-sm font-semibold text-(--accent)`}
          disabled={formDisabled}
          type="submit"
          form="compact-sync-settings-form"
        >
          {t(language, "compact.navigation.done")}
        </button>
      </header>

      <div
        className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden px-4 py-5 pb-[calc(1.25rem+var(--compact-bottom-inset))]"
        data-compact-scroll="vertical"
      >
        <form
          className="mx-auto grid w-full min-w-0 max-w-full gap-5 sm:max-w-lg"
          id="compact-sync-settings-form"
          onSubmit={(event) => {
            event.preventDefault();
            done().catch(reportOperationFailure);
          }}
        >
          <p className="m-0 min-w-0 break-words text-xs leading-5 text-(--text-secondary)">
            {t(language, "settings.sync.plaintextDescription")}
          </p>

          {beginPending ? (
            <p className="m-0 text-sm text-(--text-secondary)" role="status">
              {t(language, "settings.sync.loading")}
            </p>
          ) : null}

          <label className="flex min-h-11 min-w-0 items-center justify-between gap-3 text-sm font-medium">
            <span className="min-w-0 break-words">{t(language, "settings.sync.enabled")}</span>
            <input
              aria-label={t(language, "settings.sync.enabled")}
              checked={draft.enabled}
              className="min-h-11 min-w-0 max-w-full accent-(--accent)"
              disabled={fieldsDisabled}
              type="checkbox"
              onChange={(event) => update({ field: "enabled", value: event.currentTarget.checked })}
            />
          </label>

          <Field label={t(language, "settings.sync.provider")}>
            <select
              aria-label={t(language, "settings.sync.provider")}
              className={inputClass}
              disabled={fieldsDisabled}
              value={draft.provider}
              onChange={(event) => update({
                field: "provider",
                value: event.currentTarget.value === "s3" ? "s3" : "webdav"
              })}
            >
              <option value="webdav">WebDAV</option>
              <option value="s3">S3</option>
            </select>
          </Field>

          <Field label={t(language, "settings.sync.remotePath")}>
            <div className="grid min-w-0 gap-1.5">
              <input
                aria-label={t(language, "settings.sync.remotePath")}
                autoComplete="off"
                className={inputClass}
                disabled={fieldsDisabled}
                value={draft.remoteRoot}
                onChange={(event) => update({ field: "remoteRoot", value: event.currentTarget.value })}
              />
              <p className="m-0 min-w-0 break-words text-xs leading-5 text-(--text-secondary)">
                {t(language, "settings.sync.remotePathDescription")}
              </p>
            </div>
          </Field>

          {draft.provider === "webdav" ? (
            <div className="grid min-w-0 gap-4">
              <Field label={t(language, "settings.sync.webdavUrl")}>
                <input
                  aria-label={t(language, "settings.sync.webdavUrl")}
                  autoCapitalize="none"
                  autoComplete="url"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  inputMode="url"
                  type="url"
                  value={draft.webdav.serverUrl}
                  onChange={(event) => update({ field: "webdav.serverUrl", value: event.currentTarget.value })}
                />
              </Field>
              <Field label={t(language, "settings.sync.username")}>
                <input
                  aria-label={t(language, "settings.sync.username")}
                  autoCapitalize="none"
                  autoComplete="username"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  value={draft.webdav.username}
                  onChange={(event) => update({ field: "webdav.username", value: event.currentTarget.value })}
                />
              </Field>
              <Field label={t(language, "settings.sync.password")}>
                <input
                  aria-label={t(language, "settings.sync.password")}
                  autoComplete="current-password"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  type="password"
                  value={draft.webdav.password}
                  onChange={(event) => update({ field: "webdav.password", value: event.currentTarget.value })}
                />
              </Field>
            </div>
          ) : (
            <div className="grid min-w-0 gap-4">
              <Field label={t(language, "settings.sync.s3EndpointUrl")}>
                <input
                  aria-label={t(language, "settings.sync.s3EndpointUrl")}
                  autoCapitalize="none"
                  autoComplete="url"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  inputMode="url"
                  type="url"
                  value={draft.s3.endpointUrl}
                  onChange={(event) => update({ field: "s3.endpointUrl", value: event.currentTarget.value })}
                />
              </Field>
              <Field label={t(language, "settings.sync.s3Region")}>
                <input
                  aria-label={t(language, "settings.sync.s3Region")}
                  autoCapitalize="none"
                  autoComplete="off"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  value={draft.s3.region}
                  onChange={(event) => update({ field: "s3.region", value: event.currentTarget.value })}
                />
              </Field>
              <Field label={t(language, "settings.sync.s3Bucket")}>
                <input
                  aria-label={t(language, "settings.sync.s3Bucket")}
                  autoCapitalize="none"
                  autoComplete="off"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  value={draft.s3.bucket}
                  onChange={(event) => update({ field: "s3.bucket", value: event.currentTarget.value })}
                />
              </Field>
              <Field label={t(language, "settings.sync.s3AccessKeyId")}>
                <input
                  aria-label={t(language, "settings.sync.s3AccessKeyId")}
                  autoCapitalize="none"
                  autoComplete="off"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  type="password"
                  value={draft.s3.accessKeyId}
                  onChange={(event) => update({ field: "s3.accessKeyId", value: event.currentTarget.value })}
                />
              </Field>
              <Field label={t(language, "settings.sync.s3SecretAccessKey")}>
                <input
                  aria-label={t(language, "settings.sync.s3SecretAccessKey")}
                  autoComplete="off"
                  className={inputClass}
                  disabled={fieldsDisabled}
                  type="password"
                  value={draft.s3.secretAccessKey}
                  onChange={(event) => update({ field: "s3.secretAccessKey", value: event.currentTarget.value })}
                />
              </Field>
              <Field label={t(language, "settings.sync.s3RequestTimeout")}>
                <input
                  aria-label={t(language, "settings.sync.s3RequestTimeout")}
                  className={inputClass}
                  disabled={fieldsDisabled}
                  inputMode="numeric"
                  max={600}
                  min={5}
                  type="number"
                  value={draft.s3.requestTimeoutSeconds}
                  onChange={(event) => update({
                    field: "s3.requestTimeoutSeconds",
                    value: Math.min(600, Math.max(5, Number(event.currentTarget.value) || 5))
                  })}
                />
              </Field>
              <Field label={t(language, "settings.sync.s3AddressingStyle")}>
                <select
                  aria-label={t(language, "settings.sync.s3AddressingStyle")}
                  className={inputClass}
                  disabled={fieldsDisabled}
                  value={draft.s3.addressingStyle}
                  onChange={(event) => update({
                    field: "s3.addressingStyle",
                    value: event.currentTarget.value === "path"
                      ? "path"
                      : event.currentTarget.value === "virtual-hosted"
                        ? "virtual-hosted"
                        : "auto"
                  })}
                >
                  <option value="auto">{t(language, "settings.sync.s3AddressingStyle.auto")}</option>
                  <option value="path">{t(language, "settings.sync.s3AddressingStyle.path")}</option>
                  <option value="virtual-hosted">{t(language, "settings.sync.s3AddressingStyle.virtualHosted")}</option>
                </select>
              </Field>
              <Field label={t(language, "settings.sync.s3TlsVerification")}>
                <select
                  aria-label={t(language, "settings.sync.s3TlsVerification")}
                  className={inputClass}
                  disabled={fieldsDisabled}
                  value={draft.s3.tlsVerification}
                  onChange={(event) => update({
                    field: "s3.tlsVerification",
                    value: event.currentTarget.value === "skip" ? "skip" : "verify"
                  })}
                >
                  <option value="verify">{t(language, "settings.sync.s3TlsVerification.verify")}</option>
                  <option value="skip">{t(language, "settings.sync.s3TlsVerification.skip")}</option>
                </select>
              </Field>
            </div>
          )}

          <label className="flex min-h-11 min-w-0 items-center justify-between gap-3 text-sm font-medium">
            <span className="min-w-0 break-words">{t(language, "compact.sync.autoSyncOnSave")}</span>
            <input
              aria-label={t(language, "compact.sync.autoSyncOnSave")}
              checked={draft.autoSyncOnSave}
              className="min-h-11 min-w-0 max-w-full accent-(--accent)"
              disabled={fieldsDisabled}
              type="checkbox"
              onChange={(event) => update({ field: "autoSyncOnSave", value: event.currentTarget.checked })}
            />
          </label>

          <Field label={t(language, "settings.sync.intervalMinutes")}>
            <input
              aria-label={t(language, "settings.sync.intervalMinutes")}
              className={inputClass}
              disabled={fieldsDisabled}
              inputMode="numeric"
              max={1440}
              min={0}
              type="number"
              value={draft.intervalMinutes}
              onChange={(event) => update({
                field: "intervalMinutes",
                value: Math.min(1440, Math.max(0, Number(event.currentTarget.value) || 0))
              })}
            />
          </Field>

          {mode === "recover" ? null : (
            <button
              className={`${targetClass} inline-flex w-full items-center justify-center gap-2 rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-4 text-sm font-semibold disabled:opacity-50`}
              disabled={!canTest}
              type="button"
              onClick={() => testConnection().catch(reportOperationFailure)}
            >
              <TestTube2 aria-hidden="true" size={18} />
              {t(language, controller.testing ? "settings.sync.testingConnection" : "compact.sync.testConnection")}
            </button>
          )}

          {connectionState === "succeeded" ? (
            <p className="m-0 flex min-w-0 items-center gap-2 break-words text-sm text-(--status-success)" role="status">
              <Check aria-hidden="true" size={18} />
              {t(language, "settings.sync.connectionSucceeded")}
            </p>
          ) : null}
          {connectionState === "failed" ? (
            <p className="m-0 min-w-0 break-words text-sm text-(--status-error)" role="alert">
              {t(language, "settings.sync.connectionFailed")}
            </p>
          ) : null}
          {errorVisible ? (
            <p className="m-0 min-w-0 whitespace-normal break-words text-sm text-(--status-error)" role="alert">
              {t(language, "compact.sync.formError")}
            </p>
          ) : null}
        </form>
      </div>
    </section>
  );
}
