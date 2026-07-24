/* Hallmark · pre-emit critique: P5 H5 E5 S5 R5 V4 */
/* Hallmark · component: mobile notebook sheet · genre: editorial · theme: locked QingYu
 * states: default · hover · focus · active · disabled · loading · error · success
 * contrast: pass (46–50)
 */

import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { Cloud, Folder, LoaderCircle, Plus, RefreshCw } from "lucide-react";
import { t, type AppLanguage } from "@markra/shared";
import type { RemoteNotebookCatalogEntry } from "../../runtime";
import { isValidManagedNotebookName } from "../../lib/settings/local-state";
import { containDialogTabFocus } from "./dialog-focus";
import { remoteNotebookDisabledReasonKey } from "./remote-notebook-disabled-reason";

export type MobileNotebookDialogProps = {
  error: string | null;
  language?: AppLanguage;
  loading: boolean;
  localNames: readonly string[];
  onCancel: () => unknown;
  onCreate: (name: string) => Promise<unknown>;
  onRefresh: () => Promise<unknown>;
  onRestore: (name: string) => Promise<unknown>;
  onSwitch: (name: string) => Promise<unknown>;
  remoteEntries: readonly RemoteNotebookCatalogEntry[];
};

type MobileOperation = "create" | "refresh" | "restore" | "switch" | null;

const mobileButtonClass = "min-h-11 min-w-11 whitespace-nowrap rounded-md border border-(--border-default) px-3 text-[12px] font-[620] text-(--text-heading) transition-colors duration-150 hover:bg-(--bg-hover) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) active:translate-y-px disabled:cursor-default disabled:opacity-60 motion-reduce:transform-none motion-reduce:transition-none";

export function MobileNotebookDialog({
  error,
  language = "en",
  loading,
  localNames,
  onCancel,
  onCreate,
  onRefresh,
  onRestore,
  onSwitch,
  remoteEntries
}: MobileNotebookDialogProps) {
  const label = (key: string) => t(language, key);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const [name, setName] = useState("");
  const [selectedRemoteName, setSelectedRemoteName] = useState<string | null>(null);
  const [operation, setOperation] = useState<MobileOperation>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const invalidName = name.length > 0 && !isValidManagedNotebookName(name);
  const busy = loading || operation !== null;

  useEffect(() => {
    previousFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    dialogRef.current?.focus();
    return () => previousFocusRef.current?.focus();
  }, []);

  useEffect(() => {
    if (selectedRemoteName === null) return;
    const selected = remoteEntries.find((catalogEntry) => catalogEntry.name === selectedRemoteName);
    if (!selected?.available) setSelectedRemoteName(null);
  }, [remoteEntries, selectedRemoteName]);

  const run = async (nextOperation: Exclude<MobileOperation, null>, action: () => Promise<unknown>) => {
    if (busy) return;
    setOperationError(null);
    setOperation(nextOperation);
    try {
      await action();
    } catch {
      setOperationError(label("notebooks.mobile.operationError"));
    } finally {
      setOperation(null);
    }
  };

  const submitCreate = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!name || invalidName || busy) return;
    run("create", () => onCreate(name)).catch(() => {});
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (containDialogTabFocus(event, dialogRef.current)) return;
    if (event.key !== "Escape" || operation !== null) return;
    event.preventDefault();
    onCancel();
  };

  const visibleError = operationError ?? error;
  const selectedRemoteExistsLocally = selectedRemoteName !== null && localNames.includes(selectedRemoteName);

  return (
    <div className="fixed inset-0 z-50 flex items-end bg-[color-mix(in_srgb,var(--text-heading)_42%,transparent)] pt-[max(1rem,var(--compact-safe-area-top))]">
      <div
        ref={dialogRef}
        aria-busy={operation !== null}
        aria-labelledby="mobile-notebook-dialog-title"
        aria-modal="true"
        className="grid max-h-[calc(100dvh-1rem)] w-full grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-t-lg border border-b-0 border-(--border-default) bg-(--bg-primary) pb-[var(--compact-safe-area-bottom)] shadow-xl focus:outline-none"
        role="dialog"
        tabIndex={-1}
        onKeyDown={handleKeyDown}
      >
        <header className="flex min-h-14 items-center justify-between gap-3 border-b border-(--border-default) px-4 py-2">
          <h2 className="m-0 text-[14px] font-bold text-(--text-heading)" id="mobile-notebook-dialog-title">
            {label("notebooks.mobile.title")}
          </h2>
          <button
            className={mobileButtonClass}
            disabled={operation !== null}
            type="button"
            onClick={onCancel}
          >
            {label("notebooks.action.cancel")}
          </button>
        </header>

        <div className="grid min-h-0 content-start gap-5 overflow-y-auto px-4 py-4">
          <form className="grid gap-2" onSubmit={submitCreate}>
            <label className="grid gap-1 text-[12px] font-[620] text-(--text-heading)">
              {label("notebooks.mobile.name")}
              <input
                aria-invalid={invalidName || undefined}
                className="min-h-11 rounded-md border border-(--border-default) bg-(--bg-primary) px-3 text-[16px] text-(--text-heading) outline-2 outline-transparent hover:border-(--border-strong) focus-visible:border-(--accent) focus-visible:outline-(--accent) focus-visible:outline-offset-1 aria-invalid:border-(--danger) disabled:cursor-default disabled:opacity-60"
                disabled={busy}
                type="text"
                value={name}
                onChange={(event) => setName(event.currentTarget.value)}
              />
            </label>
            <p
              aria-hidden={invalidName ? undefined : "true"}
              className="m-0 min-h-5 text-[12px] leading-5 text-(--danger)"
              role={invalidName ? "alert" : undefined}
            >
              {invalidName ? label("notebooks.mobile.invalidName") : "\u00a0"}
            </p>
            <button
              className={`${mobileButtonClass} justify-self-start bg-(--accent) text-(--bg-primary) hover:bg-(--accent-hover)`}
              disabled={!name || invalidName || busy}
              type="submit"
            >
              <Plus aria-hidden="true" size={15} />
              {label("notebooks.action.create")}
            </button>
          </form>

          <section className="grid gap-2" aria-labelledby="mobile-local-notebooks-title">
            <div className="flex items-center gap-2 text-(--text-heading)">
              <Folder aria-hidden="true" size={16} strokeWidth={1.7} />
              <h3 className="m-0 text-[12px] font-bold" id="mobile-local-notebooks-title">
                {label("notebooks.mobile.localTitle")}
              </h3>
            </div>
            {localNames.length === 0 ? (
              <p className="m-0 text-[12px] leading-5 text-(--text-primary)">
                {label("notebooks.mobile.localEmpty")}
              </p>
            ) : (
              <ul
                aria-label={label("notebooks.mobile.localListLabel")}
                className="m-0 grid list-none gap-1 p-0"
              >
                {localNames.map((localName) => (
                  <li key={localName}>
                    <button
                      className={`${mobileButtonClass} w-full justify-start overflow-hidden text-left`}
                      disabled={busy}
                      type="button"
                      onClick={() => run("switch", () => onSwitch(localName)).catch(() => {})}
                    >
                      <span className="min-w-0 truncate">{localName}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="grid gap-2" aria-labelledby="mobile-remote-notebooks-title">
            <div className="flex items-center gap-2 text-(--text-heading)">
              <Cloud aria-hidden="true" size={16} strokeWidth={1.7} />
              <h3 className="m-0 text-[12px] font-bold" id="mobile-remote-notebooks-title">
                {label("notebooks.mobile.remoteTitle")}
              </h3>
            </div>
            {busy && operation !== "create" && operation !== "switch" ? (
              <p className="m-0 flex min-h-11 items-center gap-2 text-[12px] text-(--text-primary)" role="status">
                <LoaderCircle
                  aria-hidden="true"
                  className="animate-spin motion-reduce:animate-none"
                  size={16}
                />
                {operation === "restore"
                  ? label("notebooks.remote.restoring")
                  : label("notebooks.remote.loading")}
              </p>
            ) : remoteEntries.length === 0 ? (
              <p className="m-0 text-[12px] leading-5 text-(--text-primary)">
                {label("notebooks.remote.empty")}
              </p>
            ) : (
              <ul
                aria-label={label("notebooks.mobile.remoteListLabel")}
                className="m-0 grid list-none gap-1 p-0"
              >
                {remoteEntries.map((remoteEntry) => (
                  <li className="grid gap-1" key={remoteEntry.name}>
                    <button
                      aria-pressed={selectedRemoteName === remoteEntry.name}
                      className={`${mobileButtonClass} w-full justify-start overflow-hidden text-left aria-pressed:border-(--accent) aria-pressed:bg-(--bg-active)`}
                      disabled={busy || !remoteEntry.available}
                      type="button"
                      onClick={() => setSelectedRemoteName(remoteEntry.name)}
                    >
                      <span className="min-w-0 truncate">{remoteEntry.name}</span>
                    </button>
                    {!remoteEntry.available && remoteEntry.disabledReason ? (
                      <p className="m-0 px-3 text-[11px] leading-4 text-(--text-primary)">
                        {label(remoteNotebookDisabledReasonKey(remoteEntry.disabledReason))}
                      </p>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
            {selectedRemoteExistsLocally && !busy ? (
              <p className="m-0 rounded-md border border-(--border-default) bg-(--bg-secondary) px-3 py-2 text-[11px] leading-5 text-(--text-primary)">
                {label("notebooks.mobile.mergeWarning")}
              </p>
            ) : null}
            {visibleError ? (
              <p className="m-0 text-[12px] leading-5 text-(--danger)" role="alert">
                {visibleError}
              </p>
            ) : null}
            <div className="flex flex-wrap gap-2">
              {visibleError ? (
                <button
                  className={mobileButtonClass}
                  disabled={busy}
                  type="button"
                  onClick={() => run("refresh", onRefresh).catch(() => {})}
                >
                  <RefreshCw aria-hidden="true" size={15} />
                  {label("notebooks.action.retry")}
                </button>
              ) : null}
              <button
                className={`${mobileButtonClass} bg-(--accent) text-(--bg-primary) hover:bg-(--accent-hover)`}
                disabled={busy || selectedRemoteName === null}
                type="button"
                onClick={() => {
                  if (selectedRemoteName === null) return;
                  run("restore", () => onRestore(selectedRemoteName)).catch(() => {});
                }}
              >
                {label("notebooks.action.restore")}
              </button>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
