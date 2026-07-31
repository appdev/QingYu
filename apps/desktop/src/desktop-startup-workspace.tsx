import { useMemo, useSyncExternalStore } from "react";

export interface DesktopStartupWorkspaceProps {
  readonly selectWorkspace: () => Promise<string | null>;
  readonly startWorkspace: (workspacePath: string) => Promise<unknown>;
}

export type DesktopStartupWorkspaceState =
  | { readonly status: "unselected" }
  | { readonly status: "selecting" }
  | {
      readonly status: "starting" | "retrying" | "failed";
      readonly workspacePath: string | null;
    };

export interface DesktopStartupWorkspaceController {
  readonly getSnapshot: () => DesktopStartupWorkspaceState;
  readonly retry: () => Promise<undefined>;
  readonly select: () => Promise<undefined>;
  readonly subscribe: (
    listener: () => unknown
  ) => () => undefined;
}

export function createDesktopStartupWorkspaceController(
  options: DesktopStartupWorkspaceProps
): DesktopStartupWorkspaceController {
  const listeners = new Set<() => unknown>();
  let state: DesktopStartupWorkspaceState = { status: "unselected" };
  let pending: Promise<undefined> | undefined;
  let requestRevision = 0;

  const publish = (nextState: DesktopStartupWorkspaceState) => {
    state = nextState;
    for (const listener of [...listeners]) listener();
    return undefined;
  };

  const select = () => {
    if (pending !== undefined) return pending;
    if (state.status !== "unselected" && state.status !== "failed") {
      return Promise.resolve(undefined);
    }
    const revision = ++requestRevision;
    publish({ status: "selecting" });

    let selection: Promise<string | null>;
    try {
      selection = options.selectWorkspace();
    } catch {
      if (requestRevision === revision) {
        publish({ status: "failed", workspacePath: null });
      }
      return Promise.resolve(undefined);
    }

    let operation!: Promise<undefined>;
    operation = selection.then(
      async (workspacePath) => {
        if (requestRevision !== revision) return undefined;
        if (workspacePath === null) {
          publish({ status: "unselected" });
          return undefined;
        }
        publish({ status: "starting", workspacePath });
        try {
          await options.startWorkspace(workspacePath);
        } catch {
          if (requestRevision === revision) {
            publish({ status: "failed", workspacePath });
          }
        }
        return undefined;
      },
      () => {
        if (requestRevision === revision) {
          publish({ status: "failed", workspacePath: null });
        }
        return undefined;
      }
    ).then(() => {
      if (pending === operation) pending = undefined;
      return undefined;
    });
    pending = operation;
    return operation;
  };

  const retry = () => {
    if (pending !== undefined) return pending;
    if (state.status !== "failed" || state.workspacePath === null) {
      return Promise.resolve(undefined);
    }
    const workspacePath = state.workspacePath;
    const revision = ++requestRevision;
    publish({ status: "retrying", workspacePath });

    let startup: Promise<unknown>;
    try {
      startup = options.startWorkspace(workspacePath);
    } catch {
      if (requestRevision === revision) {
        publish({ status: "failed", workspacePath });
      }
      return Promise.resolve(undefined);
    }

    let operation!: Promise<undefined>;
    operation = startup.then(
      () => undefined,
      () => requestRevision === revision
        ? publish({ status: "failed", workspacePath })
        : undefined
    ).then(() => {
      if (pending === operation) pending = undefined;
      return undefined;
    });
    pending = operation;
    return operation;
  };

  return Object.freeze({
    getSnapshot: () => state,
    retry,
    select,
    subscribe: (listener: () => unknown) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) {
          requestRevision += 1;
          pending = undefined;
        }
        return undefined;
      };
    }
  });
}

export function DesktopStartupWorkspace(
  props: DesktopStartupWorkspaceProps
) {
  const controller = useMemo(
    () => createDesktopStartupWorkspaceController(props),
    [props.selectWorkspace, props.startWorkspace]
  );
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot
  );
  const selecting = state.status === "selecting";
  const starting = state.status === "starting";
  const retrying = state.status === "retrying";
  const busy = starting || retrying;
  const failed = state.status === "failed";
  const retryable = failed && state.workspacePath !== null;
  const selectionFailed = failed && !retryable;

  return (
    <main
      className="welcome-screen welcome-screen--desktop"
      data-desktop-startup-workspace={state.status}
    >
      <aside className="welcome-screen__identity" aria-label="QingYu">
        <p className="welcome-screen__wordmark">QingYu</p>
        <p className="welcome-screen__slogan">Write clearly. Keep it yours.</p>
        <p className="welcome-screen__promise">
          Your workspace stays local and opens through the native Kernel.
        </p>
      </aside>

      <section className="welcome-screen__desktop-task" aria-labelledby="desktop-startup-title">
        <div className="welcome-screen__desktop-content">
          <div className="welcome-screen__task-copy">
            <h1 id="desktop-startup-title">
              {busy
                ? "Preparing QingYu"
                : failed
                  ? selectionFailed
                    ? "QingYu could not choose a workspace"
                    : "QingYu could not open this workspace"
                  : "Choose your workspace"}
            </h1>
            <p role={failed ? "alert" : undefined}>
              {busy
                ? retrying
                  ? "QingYu is retrying the native Kernel without changing your workspace."
                  : "The native Kernel is opening your workspace."
                : failed
                  ? selectionFailed
                    ? "The directory selector could not open. Try choosing the directory again."
                    : "The native Kernel could not start for this workspace. Retry or choose another directory."
                  : "Select the local notebook directory QingYu should open."}
            </p>
          </div>
          {busy ? (
            <div className="welcome-screen__status" role="status">
              <span
                aria-hidden="true"
                className="welcome-screen__spinner h-4 w-4 rounded-full border-2 border-(--border-strong) border-t-(--accent)"
              />
              <span>
                {retrying
                  ? "Retrying native Kernel startup…"
                  : "Starting the native Kernel for your workspace…"}
              </span>
            </div>
          ) : failed ? (
            <div className="welcome-screen__primary-actions">
              {retryable ? (
                <button
                  className="welcome-screen__action inline-flex cursor-pointer items-center justify-center rounded-md border border-(--accent) bg-(--accent) px-4 text-[13px] font-[700] text-white"
                  type="button"
                  onClick={controller.retry}
                >
                  Retry
                </button>
              ) : null}
              <button
                className={retryable
                  ? "welcome-screen__action inline-flex cursor-pointer items-center justify-center rounded-md border border-(--border-strong) bg-(--bg-primary) px-4 text-[13px] font-[650] text-(--text-primary)"
                  : "welcome-screen__action inline-flex cursor-pointer items-center justify-center rounded-md border border-(--accent) bg-(--accent) px-4 text-[13px] font-[700] text-white"}
                type="button"
                onClick={controller.select}
              >
                {retryable ? "Choose another directory" : "Choose directory"}
              </button>
            </div>
          ) : (
            <div className="welcome-screen__primary-actions">
              <button
                className="welcome-screen__action inline-flex cursor-pointer items-center justify-center rounded-md border border-(--accent) bg-(--accent) px-4 text-[13px] font-[700] text-white disabled:cursor-default disabled:opacity-70"
                disabled={selecting}
                type="button"
                onClick={controller.select}
              >
                {selecting ? "Choosing directory…" : "Choose directory"}
              </button>
            </div>
          )}
        </div>
      </section>
    </main>
  );
}
