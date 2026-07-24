/* Hallmark · pre-emit critique: P5 H5 E5 S5 R5 V4 */
/* Hallmark · component: notebook picker · genre: editorial · theme: locked QingYu
 * states: default · hover · focus · active · disabled · loading · error · success
 * contrast: pass (46–50)
 */

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Cloud, LoaderCircle, RefreshCw } from "lucide-react";
import { t, type AppLanguage } from "@markra/shared";
import { Button } from "@markra/ui";
import type { RemoteNotebookCatalogEntry } from "../../runtime";
import { containDialogTabFocus } from "./dialog-focus";
import { remoteNotebookDisabledReasonKey } from "./remote-notebook-disabled-reason";

export type RemoteNotebookDialogProps = {
  allowCurrentNotebookSelection?: boolean;
  currentNotebookName?: string | null;
  entries: readonly RemoteNotebookCatalogEntry[];
  error: string | null;
  language?: AppLanguage;
  loading: boolean;
  onCancel: () => unknown;
  onRefresh: () => Promise<unknown>;
  onRestore: (name: string) => Promise<unknown>;
};

export function RemoteNotebookDialog({
  allowCurrentNotebookSelection = false,
  currentNotebookName = null,
  entries,
  error,
  language = "en",
  loading,
  onCancel,
  onRefresh,
  onRestore
}: RemoteNotebookDialogProps) {
  const label = (key: string) => t(language, key);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const busy = loading || refreshing || restoring;
  const selectedEntry = selectedName === null
    ? null
    : entries.find((catalogEntry) => catalogEntry.name === selectedName) ?? null;
  const selectedCurrentNotebook = selectedEntry?.name === currentNotebookName;
  const isSelectedRestorable = selectedEntry?.available === true && (
    allowCurrentNotebookSelection || !selectedCurrentNotebook
  );

  useEffect(() => {
    previousFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    dialogRef.current?.focus();
    return () => previousFocusRef.current?.focus();
  }, []);

  useEffect(() => {
    if (selectedName === null) return;
    if (!isSelectedRestorable) setSelectedName(null);
  }, [isSelectedRestorable, selectedName]);

  const refresh = async () => {
    if (busy) return;
    setOperationError(null);
    setRefreshing(true);
    try {
      await onRefresh();
    } catch {
      setOperationError(label("notebooks.remote.refreshError"));
    } finally {
      setRefreshing(false);
    }
  };

  const restore = async () => {
    if (busy || !isSelectedRestorable || !selectedEntry) return;
    setOperationError(null);
    setRestoring(true);
    try {
      await onRestore(selectedEntry.name);
    } catch {
      setOperationError(label("notebooks.remote.restoreError"));
    } finally {
      setRestoring(false);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (containDialogTabFocus(event, dialogRef.current)) return;
    if (event.key !== "Escape" || restoring || refreshing) return;
    event.preventDefault();
    onCancel();
  };

  const visibleError = operationError ?? error;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[color-mix(in_srgb,var(--text-heading)_42%,transparent)] px-4 py-6">
      <div
        ref={dialogRef}
        aria-busy={restoring}
        aria-labelledby="remote-notebook-dialog-title"
        aria-modal="true"
        className="grid max-h-[min(40rem,calc(100dvh-3rem))] w-full max-w-xl grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden rounded-lg border border-(--border-default) bg-(--bg-primary) shadow-xl focus:outline-none"
        role="dialog"
        tabIndex={-1}
        onKeyDown={handleKeyDown}
      >
        <header className="grid gap-1 border-b border-(--border-default) px-4 py-4">
          <div className="flex items-center gap-2 text-(--text-heading)">
            <Cloud aria-hidden="true" size={17} strokeWidth={1.7} />
            <h2 className="m-0 text-[13px] leading-5 font-bold" id="remote-notebook-dialog-title">
              {label("notebooks.remote.title")}
            </h2>
          </div>
          <p className="m-0 text-[12px] leading-5 text-(--text-primary)">
            {label("notebooks.remote.description")}
          </p>
        </header>

        <div className="grid min-h-0 content-start gap-3 overflow-y-auto px-4 py-4">
          {loading || refreshing ? (
            <p className="m-0 flex min-h-11 items-center gap-2 text-[12px] text-(--text-primary)" role="status">
              <LoaderCircle
                aria-hidden="true"
                className="animate-spin motion-reduce:animate-none"
                size={16}
              />
              {label("notebooks.remote.loading")}
            </p>
          ) : entries.length === 0 && !visibleError ? (
            <p className="m-0 py-6 text-center text-[12px] leading-5 text-(--text-primary)">
              {label("notebooks.remote.empty")}
            </p>
          ) : (
            <fieldset className="m-0 grid min-w-0 gap-1 border-0 p-0">
              <legend className="sr-only">{label("notebooks.remote.listLabel")}</legend>
              {entries.map((catalogEntry) => {
                const currentNotebook = catalogEntry.name === currentNotebookName;
                return (
                  <label
                    className="grid min-h-11 cursor-pointer grid-cols-[auto_minmax(0,1fr)] items-start gap-x-3 rounded-md border border-transparent px-3 py-2 text-[13px] text-(--text-heading) hover:bg-(--bg-hover) active:bg-(--bg-active) has-[:checked]:border-(--border-default) has-[:checked]:bg-(--bg-active) has-[:disabled]:cursor-default has-[:disabled]:opacity-60"
                    key={catalogEntry.name}
                  >
                    <input
                      aria-label={catalogEntry.name}
                      checked={selectedName === catalogEntry.name}
                      className="mt-1 accent-(--accent) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
                      disabled={busy || !catalogEntry.available || (
                        currentNotebook && !allowCurrentNotebookSelection
                      )}
                      name="remote-notebook"
                      type="radio"
                      value={catalogEntry.name}
                      onChange={() => setSelectedName(catalogEntry.name)}
                    />
                    <span className="min-w-0 overflow-wrap-anywhere font-[620]">
                      {catalogEntry.name}
                    </span>
                    {currentNotebook ? (
                      <span className="col-start-2 text-[11px] leading-4 text-(--text-primary)">
                        {label("settings.sync.primaryRoot")}
                      </span>
                    ) : !catalogEntry.available && catalogEntry.disabledReason ? (
                      <span className="col-start-2 text-[11px] leading-4 text-(--text-primary)">
                        {label(remoteNotebookDisabledReasonKey(catalogEntry.disabledReason))}
                      </span>
                    ) : null}
                  </label>
                );
              })}
            </fieldset>
          )}

          {restoring ? (
            <p className="m-0 flex min-h-11 items-center gap-2 text-[12px] text-(--text-primary)" role="status">
              <LoaderCircle
                aria-hidden="true"
                className="animate-spin motion-reduce:animate-none"
                size={16}
              />
              {label("notebooks.remote.restoring")}
            </p>
          ) : null}

          {isSelectedRestorable && !busy ? (
            <p className="m-0 rounded-md border border-(--border-default) bg-(--bg-secondary) px-3 py-2 text-[11px] leading-5 text-(--text-primary)">
              {label("notebooks.remote.mergeWarning")}
            </p>
          ) : null}
          {visibleError ? (
            <p className="m-0 text-[12px] leading-5 text-(--danger)" role="alert">
              {visibleError}
            </p>
          ) : null}
        </div>

        <footer className="flex flex-wrap justify-end gap-2 border-t border-(--border-default) px-4 py-3">
          {visibleError ? (
            <Button
              className="min-h-11 min-w-11 whitespace-nowrap active:translate-y-px motion-reduce:transform-none"
              disabled={busy}
              onClick={() => refresh().catch(() => {})}
            >
              <RefreshCw aria-hidden="true" size={14} />
              {label("notebooks.action.retry")}
            </Button>
          ) : null}
          <Button
            className="min-h-11 min-w-11 whitespace-nowrap active:translate-y-px motion-reduce:transform-none"
            disabled={restoring || refreshing}
            onClick={onCancel}
          >
            {label("notebooks.action.cancel")}
          </Button>
          <Button
            className="min-h-11 min-w-11 whitespace-nowrap active:translate-y-px motion-reduce:transform-none"
            disabled={busy || !isSelectedRestorable}
            variant="primary"
            onClick={() => restore().catch(() => {})}
          >
            {label(allowCurrentNotebookSelection && selectedCurrentNotebook
              ? "settings.sync.run"
              : "notebooks.action.restore")}
          </Button>
        </footer>
      </div>
    </div>
  );
}
