import { History, LoaderCircle, X } from "lucide-react";
import { useEffect, useState } from "react";
import { t, type AppLanguage } from "@markra/shared";
import { Button } from "@markra/ui";
import type { ConflictVersion, ConflictVersions, SyncConflictRecord } from "../../lib/sync-config";

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

export function SyncConflictHistoryDialog({
  conflict,
  language,
  onClose,
  onRead
}: {
  conflict: SyncConflictRecord;
  language: AppLanguage;
  onClose: () => unknown;
  onRead: (conflict: SyncConflictRecord) => Promise<ConflictVersions>;
}) {
  const label = (key: Parameters<typeof t>[1]) => t(language, key);
  const [versions, setVersions] = useState<ConflictVersions | null>(null);
  const [loadError, setLoadError] = useState(false);

  useEffect(() => {
    let active = true;
    setVersions(null);
    setLoadError(false);
    onRead(conflict).then((next) => {
      if (active) setVersions(next);
    }).catch(() => {
      if (active) setLoadError(true);
    });
    return () => {
      active = false;
    };
  }, [conflict, onRead]);

  return (
    <section
      aria-labelledby="sync-conflict-dialog-title"
      aria-modal="false"
      className="fixed top-14 right-4 bottom-14 z-50 grid w-[min(46rem,calc(100vw-2rem))] grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-xl border border-(--border-default) bg-(--bg-primary) shadow-2xl"
      role="dialog"
    >
      <header className="flex items-start gap-3 border-b border-(--border-default) px-4 py-3">
        <History aria-hidden="true" className="mt-0.5 shrink-0 text-(--text-secondary)" size={17} />
        <div className="min-w-0 flex-1">
          <h2 className="m-0 text-[13px] font-[700] text-(--text-heading)" id="sync-conflict-dialog-title">
            {label("sync.conflict.historyTitle")}
          </h2>
          <p className="m-0 mt-1 break-all text-[12px] text-(--text-secondary)">
            {conflict.relativePath}
          </p>
        </div>
        <Button aria-label={label("sync.conflict.close")} onClick={onClose}>
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
    </section>
  );
}
