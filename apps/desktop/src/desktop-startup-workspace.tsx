import { useEffect, useRef, useSyncExternalStore } from "react";

export interface DesktopStartupWorkspaceProps {
  readonly retryWorkspace: () => Promise<unknown>;
  readonly selectWorkspace: () => Promise<string | null>;
  readonly startWorkspace: (workspacePath: string) => Promise<unknown>;
  readonly startupStatus: DesktopStartupWorkspaceAuthoritativeStatus;
}

export type DesktopStartupWorkspaceAuthoritativeStatus =
  | "unselected"
  | "invalid"
  | "unavailable"
  | "starting"
  | "ready"
  | "failed";

export type DesktopStartupWorkspaceState =
  | { readonly status: "unselected" }
  | { readonly status: "selecting" }
  | {
      readonly status: "starting" | "retrying";
      readonly workspacePath: string | null;
    }
  | {
      readonly failure: "selection" | "startup";
      readonly status: "failed";
      readonly workspacePath: string | null;
    };

export interface DesktopStartupWorkspaceController {
  readonly cancelPending: () => undefined;
  readonly getSnapshot: () => DesktopStartupWorkspaceState;
  readonly retry: () => Promise<undefined>;
  readonly select: () => Promise<undefined>;
  readonly subscribe: (
    listener: () => unknown
  ) => () => undefined;
  readonly updateOptions: (options: DesktopStartupWorkspaceProps) => undefined;
  readonly updateStartupStatus: (
    status: DesktopStartupWorkspaceAuthoritativeStatus
  ) => undefined;
}

export function createDesktopStartupWorkspaceController(
  options: DesktopStartupWorkspaceProps
): DesktopStartupWorkspaceController {
  const listeners = new Set<() => unknown>();
  let callbacks = options;
  let state = stateFromAuthoritativeStatus(options.startupStatus);
  let pending: Promise<undefined> | undefined;
  let requestRevision = 0;
  let startupStatus = options.startupStatus;

  const cancelPending = () => {
    requestRevision += 1;
    pending = undefined;
    return undefined;
  };

  const publish = (nextState: DesktopStartupWorkspaceState) => {
    state = nextState;
    for (const listener of [...listeners]) listener();
    return undefined;
  };

  const select = () => {
    if (pending !== undefined) return pending;
    if (
      state.status !== "unselected" &&
      (state.status !== "failed" || state.failure !== "selection")
    ) {
      return Promise.resolve(undefined);
    }
    const revision = ++requestRevision;
    publish({ status: "selecting" });

    let selection: Promise<string | null>;
    try {
      selection = callbacks.selectWorkspace();
    } catch {
      if (requestRevision === revision) {
        publish({ failure: "selection", status: "failed", workspacePath: null });
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
          await callbacks.startWorkspace(workspacePath);
        } catch {
          if (requestRevision === revision) {
            publish({ failure: "startup", status: "failed", workspacePath });
          }
        }
        return undefined;
      },
      () => {
        if (requestRevision === revision) {
          publish({ failure: "selection", status: "failed", workspacePath: null });
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
    if (state.status !== "failed" || state.failure !== "startup") {
      return Promise.resolve(undefined);
    }
    const workspacePath = state.workspacePath;
    const revision = ++requestRevision;
    publish({ status: "retrying", workspacePath });

    let startup: Promise<unknown>;
    try {
      startup = callbacks.retryWorkspace();
    } catch {
      if (requestRevision === revision) {
        publish({ failure: "startup", status: "failed", workspacePath });
      }
      return Promise.resolve(undefined);
    }

    let operation!: Promise<undefined>;
    operation = startup.then(
      () => undefined,
      () => requestRevision === revision
        ? publish({ failure: "startup", status: "failed", workspacePath })
        : undefined
    ).then(() => {
      if (pending === operation) pending = undefined;
      return undefined;
    });
    pending = operation;
    return operation;
  };

  const updateStartupStatus = (
    nextStatus: DesktopStartupWorkspaceAuthoritativeStatus
  ) => {
    if (startupStatus === nextStatus) return undefined;
    startupStatus = nextStatus;
    cancelPending();
    publish(stateFromAuthoritativeStatus(nextStatus));
    return undefined;
  };

  const updateOptions = (nextOptions: DesktopStartupWorkspaceProps) => {
    callbacks = nextOptions;
    return undefined;
  };

  return Object.freeze({
    cancelPending,
    getSnapshot: () => state,
    retry,
    select,
    subscribe: (listener: () => unknown) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
        return undefined;
      };
    },
    updateOptions,
    updateStartupStatus
  });
}

function stateFromAuthoritativeStatus(
  status: DesktopStartupWorkspaceAuthoritativeStatus
): DesktopStartupWorkspaceState {
  if (status === "unselected" || status === "invalid") {
    return { status: "unselected" };
  }
  if (status === "failed" || status === "unavailable") {
    return { failure: "startup", status: "failed", workspacePath: null };
  }
  return { status: "starting", workspacePath: null };
}

export function DesktopStartupWorkspace(
  props: DesktopStartupWorkspaceProps
) {
  const controllerRef = useRef<DesktopStartupWorkspaceController | null>(null);
  if (controllerRef.current === null) {
    controllerRef.current = createDesktopStartupWorkspaceController(props);
  }
  const controller = controllerRef.current;
  controller.updateOptions(props);
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot
  );
  useEffect(() => {
    controller.updateStartupStatus(props.startupStatus);
  }, [controller, props.startupStatus]);
  useEffect(() => () => controller.cancelPending(), [controller]);
  const selecting = state.status === "selecting";
  const starting = state.status === "starting";
  const retrying = state.status === "retrying";
  const busy = starting || retrying;
  const failed = state.status === "failed";
  const retryable = failed && state.failure === "startup";
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
                    : "The native Kernel could not start for this workspace. Retry when you are ready."
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
              {selectionFailed ? (
                <button
                  className="welcome-screen__action inline-flex cursor-pointer items-center justify-center rounded-md border border-(--accent) bg-(--accent) px-4 text-[13px] font-[700] text-white"
                  type="button"
                  onClick={controller.select}
                >
                  Choose directory
                </button>
              ) : null}
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
