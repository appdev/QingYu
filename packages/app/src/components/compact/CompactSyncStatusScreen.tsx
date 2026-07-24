import { AlertTriangle, ArrowLeft, Cloud, RefreshCw, Settings2 } from "lucide-react";
import { useState } from "react";
import { sanitizeDiagnosticText, t, type AppLanguage } from "@markra/shared";
import type { CompactSyncSettingsController } from "../../hooks/useCompactSyncSettings";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import type { SyncStatus } from "../../lib/sync-config";

type CompactSyncStatusScreenProps = {
  controller: CompactSyncSettingsController;
  language: AppLanguage;
  navigation: CompactNavigation;
};

const targetClass = "min-h-11 min-w-11";
const primaryButtonClass = `${targetClass} inline-flex w-full items-center justify-center gap-2 rounded-xl bg-(--accent) px-4 text-sm font-semibold text-white disabled:opacity-50`;
const secondaryButtonClass = `${targetClass} inline-flex w-full items-center justify-center gap-2 rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-4 text-sm font-semibold`;
const authorizationValuePattern = /\bauthorization\s*[:=]\s*(?:(?:basic|bearer)\s+)?[^\s,;]+/giu;
const credentialValuePattern = /\b(?:access[_ -]?key(?:id)?|secret(?:[_ -]?access[_ -]?key)?|password|token)\s*[:=]\s*[^\s,;]+/giu;

function safeSyncIssueMessage(message: string) {
  return sanitizeDiagnosticText(message)
    .replace(authorizationValuePattern, "Authorization: [redacted]")
    .replace(credentialValuePattern, "Credential: [redacted]");
}

function formatDate(value: string | null, language: AppLanguage) {
  if (!value) return t(language, "settings.sync.never");
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return t(language, "settings.sync.never");
  return new Intl.DateTimeFormat(language, {
    dateStyle: "medium",
    timeStyle: "short"
  }).format(date);
}

function statusHeading(controller: CompactSyncSettingsController, language: AppLanguage) {
  if (controller.status?.completionState === "attempting") return t(language, "compact.sync.status.attempting");
  if (controller.status?.completionState === "failed") return t(language, "compact.sync.status.failed");
  if (controller.status?.completionState === "succeeded") return t(language, "compact.sync.status.succeeded");
  if (controller.loadResult?.status === "loaded" && controller.loadResult.readiness === "disabled") {
    return t(language, "compact.sync.status.disabled");
  }
  return t(language, "compact.sync.status.ready");
}

function SafeStatusDetails({ language, status }: { language: AppLanguage; status: SyncStatus | null }) {
  if (!status) {
    return <p className="m-0 text-sm text-(--text-secondary)">{t(language, "settings.sync.status.none")}</p>;
  }

  return (
    <div className="grid min-w-0 gap-3 text-sm text-(--text-secondary)">
      <dl className="m-0 grid min-w-0 gap-2">
        <div className="flex min-w-0 justify-between gap-4">
          <dt>{t(language, "compact.sync.lastAttempt")}</dt>
          <dd className="m-0 min-w-0 text-right break-words">{formatDate(status.lastAttemptAt, language)}</dd>
        </div>
        <div className="flex min-w-0 justify-between gap-4">
          <dt>{t(language, "compact.sync.lastSuccess")}</dt>
          <dd className="m-0 min-w-0 text-right break-words">{formatDate(status.lastSuccessfulSyncAt, language)}</dd>
        </div>
      </dl>
      {status.error ? (
        <div className="min-w-0 rounded-xl bg-(--bg-secondary) p-3 break-words" role="alert">
          <p className="m-0 font-semibold text-(--status-error)">{safeSyncIssueMessage(status.error.code)}</p>
          <p className="m-0 mt-1 break-words">{safeSyncIssueMessage(status.error.operation)}</p>
          {status.error.httpStatus === null ? null : <p className="m-0">HTTP {status.error.httpStatus}</p>}
          {status.error.relativePath === null ? null : (
            <p className="m-0 break-all">{safeSyncIssueMessage(status.error.relativePath)}</p>
          )}
        </div>
      ) : null}
      {status.summary ? (
        <div className="grid min-w-0 gap-1 rounded-xl bg-(--bg-secondary) p-3">
          <p className="m-0">
            {t(language, "settings.sync.status.uploadedFiles")}: {status.summary.uploadedFiles}
          </p>
          <p className="m-0">
            {t(language, "settings.sync.status.downloadedFiles")}: {status.summary.downloadedFiles}
          </p>
          <p className="m-0">
            {t(language, "settings.sync.status.conflictFiles")}: {status.summary.conflictFiles}
          </p>
          <p className="m-0">
            {t(language, "settings.sync.status.bytesUploaded")}: {status.summary.bytesUploaded}
          </p>
          <p className="m-0">
            {t(language, "settings.sync.status.bytesDownloaded")}: {status.summary.bytesDownloaded}
          </p>
        </div>
      ) : null}
    </div>
  );
}

export function CompactSyncStatusScreen({
  controller,
  language,
  navigation
}: CompactSyncStatusScreenProps) {
  const [actionError, setActionError] = useState(false);
  const back = () => navigation.pop().catch(() => setActionError(true));
  const configure = (mode: "create" | "edit" | "recover") => navigation.push({ kind: "sync-form", mode });
  const runNow = () => {
    setActionError(false);
    controller.runImmediate().catch(() => setActionError(true));
  };
  const loadResult = controller.loadResult;

  let content;
  if (!controller.available) {
    content = (
      <div className="grid min-w-0 gap-3 text-center">
        <Cloud aria-hidden="true" className="mx-auto text-(--text-secondary)" size={32} />
        <h2 className="m-0 text-lg font-semibold">{t(language, "compact.sync.unavailableTitle")}</h2>
        <p className="m-0 break-words text-sm text-(--text-secondary)">
          {t(language, "compact.sync.unavailableDescription")}
        </p>
      </div>
    );
  } else if (!loadResult) {
    content = (
      <div className="grid min-w-0 gap-3 text-center" role="status">
        <RefreshCw aria-hidden="true" className="mx-auto animate-spin motion-reduce:animate-none" size={28} />
        <p className="m-0 text-sm text-(--text-secondary)">{t(language, "settings.sync.loading")}</p>
      </div>
    );
  } else if (!controller.primaryRoot) {
    const mode = loadResult.status === "absent"
      ? "create"
      : loadResult.status === "loaded"
        ? "edit"
        : "recover";
    content = (
      <div className="grid min-w-0 gap-4 text-center">
        <Cloud aria-hidden="true" className="mx-auto text-(--text-secondary)" size={32} />
        <div className="grid gap-2">
          <h2 className="m-0 text-lg font-semibold">
            {t(language, "compact.sync.noWorkspaceTitle")}
          </h2>
          <p className="m-0 break-words text-sm text-(--text-secondary)">
            {t(language, "compact.sync.noWorkspaceDescription")}
          </p>
        </div>
        <button
          className={primaryButtonClass}
          type="button"
          onClick={() => configure(mode)}
        >
          <Settings2 aria-hidden="true" size={18} />
          {t(language, "compact.sync.configure")}
        </button>
      </div>
    );
  } else if (loadResult.status === "absent") {
    content = (
      <div className="grid min-w-0 gap-4 text-center">
        <Cloud aria-hidden="true" className="mx-auto text-(--text-secondary)" size={32} />
        <div className="grid gap-2">
          <h2 className="m-0 text-lg font-semibold">{t(language, "compact.sync.localTitle")}</h2>
          <p className="m-0 break-words text-sm text-(--text-secondary)">
            {t(language, "compact.sync.localDescription")}
          </p>
        </div>
        <button className={primaryButtonClass} type="button" onClick={() => configure("create")}>
          <Settings2 aria-hidden="true" size={18} />
          {t(language, "compact.sync.configure")}
        </button>
      </div>
    );
  } else if (loadResult.status === "malformed" || loadResult.status === "unsupported") {
    const title = loadResult.status === "malformed"
      ? t(language, "compact.sync.malformedTitle")
      : t(language, "compact.sync.unsupportedTitle");
    content = (
      <div className="grid min-w-0 gap-4 text-center">
        <AlertTriangle aria-hidden="true" className="mx-auto text-(--status-error)" size={32} />
        <div className="grid min-w-0 gap-2">
          <h2 className="m-0 text-lg font-semibold">{title}</h2>
          <p className="m-0 min-w-0 whitespace-normal break-words text-sm text-(--text-secondary)" role="alert">
            {safeSyncIssueMessage(loadResult.issue.message)}
          </p>
        </div>
        <button className={primaryButtonClass} type="button" onClick={() => configure("recover")}>
          <Settings2 aria-hidden="true" size={18} />
          {t(language, "compact.sync.configure")}
        </button>
      </div>
    );
  } else if (loadResult.readiness === "incomplete") {
    content = (
      <div className="grid min-w-0 gap-4">
        <div className="grid gap-2 text-center">
          <AlertTriangle aria-hidden="true" className="mx-auto text-(--status-warning)" size={32} />
          <h2 className="m-0 text-lg font-semibold">{t(language, "compact.sync.status.incomplete")}</h2>
        </div>
        <ul className="m-0 grid min-w-0 gap-2 pl-5 text-sm text-(--text-secondary)">
          {loadResult.issues.map((issue) => (
            <li className="min-w-0 whitespace-normal break-words" key={`${issue.field}:${issue.code}`}>
              {safeSyncIssueMessage(issue.message)}
            </li>
          ))}
        </ul>
        <button className={primaryButtonClass} type="button" onClick={() => configure("edit")}>
          <Settings2 aria-hidden="true" size={18} />
          {t(language, "compact.sync.configure")}
        </button>
      </div>
    );
  } else {
    const syncEnabled = loadResult.config.enabled;
    const canRun = syncEnabled
      && loadResult.readiness === "ready"
      && !controller.syncRunning
      && !controller.saving;
    content = (
      <div className="grid min-w-0 gap-5">
        <div className="grid min-w-0 gap-2 text-center">
          <Cloud aria-hidden="true" className="mx-auto text-(--accent)" size={32} />
          <h2 className="m-0 text-lg font-semibold">{statusHeading(controller, language)}</h2>
          <p className="m-0 text-sm text-(--text-secondary)">
            {t(language, "compact.sync.provider")}: <span>{loadResult.config.provider === "s3" ? "S3" : "WebDAV"}</span>
          </p>
        </div>
        <SafeStatusDetails language={language} status={controller.status} />
        <div className="grid min-w-0 gap-2">
          {canRun ? (
            <button className={primaryButtonClass} type="button" onClick={runNow}>
              <RefreshCw aria-hidden="true" size={18} />
              {t(language, "compact.sync.now")}
            </button>
          ) : null}
          <button className={secondaryButtonClass} type="button" onClick={() => configure("edit")}>
            <Settings2 aria-hidden="true" size={18} />
            {t(language, "compact.sync.edit")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <section
      aria-label={t(language, "compact.sync.title")}
      className="absolute inset-0 flex h-full min-h-0 min-w-0 flex-col overflow-x-hidden bg-(--bg-primary)"
      data-testid="compact-sync-status"
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
        <h1 className="m-0 min-w-0 flex-1 truncate text-base font-semibold">{t(language, "compact.sync.title")}</h1>
      </header>
      <div
        className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden px-4 py-6 pb-[calc(1.5rem+var(--compact-bottom-inset))]"
        data-compact-scroll="vertical"
      >
        <div className="mx-auto w-full min-w-0 max-w-lg">{content}</div>
        {actionError ? (
          <p className="mx-auto mt-4 w-full max-w-lg break-words text-sm text-(--status-error)" role="alert">
            {t(language, "compact.error.sync")}
          </p>
        ) : null}
      </div>
    </section>
  );
}
