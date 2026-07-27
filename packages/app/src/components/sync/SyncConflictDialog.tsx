import { AlertTriangle, LoaderCircle, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { t, type AppLanguage } from "@markra/shared";
import { Button } from "@markra/ui";
import type {
  ConflictResolution,
  ConflictVersion,
  ConflictVersions,
  SyncConflictRecord
} from "../../lib/sync-config";

function suggestedCopyPath(relativePath: string) {
  const slash = relativePath.lastIndexOf("/");
  const directory = slash < 0 ? "" : relativePath.slice(0, slash + 1);
  const name = slash < 0 ? relativePath : relativePath.slice(slash + 1);
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return `${directory}${name}.remote`;
  return `${directory}${name.slice(0, dot)}.remote${name.slice(dot)}`;
}

function VersionPane({
  emptyLabel,
  label,
  version
}: {
  emptyLabel: string;
  label: string;
  version: ConflictVersion | null;
}) {
  return (
    <section className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-md border border-(--border-default)">
      <header className="border-b border-(--border-default) bg-(--bg-secondary) px-3 py-2 text-[12px] font-[650] text-(--text-heading)">
        {label}
      </header>
      {version?.text !== null && version ? (
        <pre className="m-0 min-h-36 overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-[12px] leading-5 text-(--text-primary)">
          {version.text}
        </pre>
      ) : (
        <div className="grid min-h-36 content-center gap-1 p-3 text-center text-[12px] text-(--text-secondary)">
          <span>{emptyLabel}</span>
          {version ? <span>{version.byteSize.toLocaleString()} B</span> : null}
        </div>
      )}
    </section>
  );
}

export function SyncConflictDialog({
  conflict,
  language,
  onClose,
  onRead,
  onResolve
}: {
  conflict: SyncConflictRecord;
  language: AppLanguage;
  onClose: () => unknown;
  onRead: (conflict: SyncConflictRecord) => Promise<ConflictVersions>;
  onResolve: (conflict: SyncConflictRecord, resolution: ConflictResolution) => Promise<unknown>;
}) {
  const label = (key: Parameters<typeof t>[1]) => t(language, key);
  const [versions, setVersions] = useState<ConflictVersions | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [pending, setPending] = useState(false);
  const [operationError, setOperationError] = useState(false);
  const [copyPath, setCopyPath] = useState(() => suggestedCopyPath(conflict.relativePath));

  useEffect(() => {
    let active = true;
    setVersions(null);
    setLoadError(false);
    setCopyPath(suggestedCopyPath(conflict.relativePath));
    onRead(conflict).then((next) => {
      if (active) setVersions(next);
    }).catch(() => {
      if (active) setLoadError(true);
    });
    return () => {
      active = false;
    };
  }, [conflict, onRead]);

  const resolve = async (resolution: ConflictResolution) => {
    if (pending) return;
    const confirmationKey = resolution.kind === "keep-local"
      ? "sync.conflict.confirmKeepLocal"
      : resolution.kind === "use-remote"
        ? "sync.conflict.confirmUseRemote"
        : "sync.conflict.confirmKeepBoth";
    if (!window.confirm(label(confirmationKey))) return;
    setPending(true);
    setOperationError(false);
    try {
      await onResolve(conflict, resolution);
      onClose();
    } catch {
      setOperationError(true);
      setPending(false);
    }
  };

  const validCopyPath = useMemo(() => {
    const value = copyPath.trim().replace(/\\/gu, "/");
    return value.length > 0
      && !value.startsWith("/")
      && !value.split("/").some((part) => part === "" || part === "." || part === "..");
  }, [copyPath]);

  return (
    <section
      aria-labelledby="sync-conflict-dialog-title"
      aria-modal="false"
      className="fixed top-14 right-4 bottom-14 z-50 grid w-[min(46rem,calc(100vw-2rem))] grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden rounded-xl border border-(--border-default) bg-(--bg-primary) shadow-2xl"
      role="dialog"
    >
      <header className="flex items-start gap-3 border-b border-(--border-default) px-4 py-3">
        <AlertTriangle aria-hidden="true" className="mt-0.5 shrink-0 text-(--danger)" size={17} />
        <div className="min-w-0 flex-1">
          <h2 className="m-0 text-[13px] font-[700] text-(--text-heading)" id="sync-conflict-dialog-title">
            {label("sync.conflict.title")}
          </h2>
          <p className="m-0 mt-1 break-all text-[12px] text-(--text-secondary)">
            {conflict.relativePath}
          </p>
        </div>
        <Button aria-label={label("sync.conflict.close")} disabled={pending} onClick={onClose}>
          <X aria-hidden="true" size={15} />
        </Button>
      </header>

      <div className="min-h-0 overflow-y-auto p-4">
        {!versions && !loadError ? (
          <p className="m-0 flex items-center gap-2 text-[12px] text-(--text-secondary)" role="status">
            <LoaderCircle aria-hidden="true" className="animate-spin" size={15} />
            {label("sync.conflict.loading")}
          </p>
        ) : loadError ? (
          <p className="m-0 text-[12px] text-(--danger)" role="alert">
            {label("sync.conflict.unavailable")}
          </p>
        ) : versions ? (
          <div className="grid min-h-full gap-3 md:grid-cols-2">
            <VersionPane
              emptyLabel={label("sync.conflict.binaryOrLarge")}
              label={label("sync.conflict.localVersion")}
              version={versions.local}
            />
            <VersionPane
              emptyLabel={label("sync.conflict.binaryOrLarge")}
              label={label("sync.conflict.remoteVersion")}
              version={versions.remote}
            />
          </div>
        ) : null}
      </div>

      <footer className="grid gap-3 border-t border-(--border-default) px-4 py-3">
        <label className="grid gap-1 text-[11px] text-(--text-secondary)">
          {label("sync.conflict.keepBothPath")}
          <input
            className="min-h-9 rounded-md border border-(--border-default) bg-(--bg-primary) px-2 text-[12px] text-(--text-heading) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
            disabled={pending || loadError}
            value={copyPath}
            onChange={(event) => setCopyPath(event.target.value)}
          />
        </label>
        {operationError ? (
          <p className="m-0 text-[12px] text-(--danger)" role="alert">
            {label("sync.conflict.resolveFailed")}
          </p>
        ) : null}
        <div className="flex flex-wrap justify-end gap-2">
          <Button disabled={pending || loadError} onClick={() => resolve({ kind: "keep-local" }).catch(() => {})}>
            {label("sync.conflict.keepLocal")}
          </Button>
          <Button disabled={pending || loadError} onClick={() => resolve({ kind: "use-remote" }).catch(() => {})}>
            {label("sync.conflict.useRemote")}
          </Button>
          <Button
            disabled={pending || loadError || !validCopyPath}
            variant="primary"
            onClick={() => resolve({
              destinationRelativePath: copyPath.trim().replace(/\\/gu, "/"),
              kind: "keep-both"
            }).catch(() => {})}
          >
            {pending ? label("sync.conflict.resolving") : label("sync.conflict.keepBoth")}
          </Button>
        </div>
      </footer>
    </section>
  );
}
