import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { platform as readTauriPlatform } from "@tauri-apps/plugin-os";
import { MacWindowControls, WindowsWindowControls } from "@markra/app";
import {
  closeNativeWindow,
  minimizeNativeWindow,
  toggleNativeWindowFullscreen,
  toggleNativeWindowMaximized,
} from "./runtime/tauri/window";

type DesktopStartupPlatform = "linux" | "macos" | "windows";

export interface DesktopStartupWorkspaceProps {
  readonly platform?: DesktopStartupPlatform | null;
  readonly retryWorkspace: () => Promise<unknown>;
  readonly selectWorkspace: () => Promise<string | null>;
  readonly startWorkspace: (workspacePath: string) => Promise<unknown>;
  readonly startupStatus: DesktopStartupWorkspaceAuthoritativeStatus;
}

function resolveDesktopStartupPlatform(): DesktopStartupPlatform | null {
  try {
    const platform = readTauriPlatform();
    return platform === "linux" || platform === "macos" || platform === "windows"
      ? platform
      : null;
  } catch {
    return null;
  }
}

function DesktopStartupWindowChrome({
  platform,
}: {
  readonly platform: DesktopStartupPlatform | null;
}) {
  if (platform === "macos") {
    return (
      <header
        aria-label="Window drag region"
        className="welcome-screen__window-chrome fixed inset-x-0 top-0 z-20 h-10 select-none [-webkit-user-select:none]"
        data-tauri-drag-region
      >
        <MacWindowControls
          className="absolute top-0 left-0 z-10"
          onClose={closeNativeWindow}
          onFullscreen={toggleNativeWindowFullscreen}
          onMinimize={minimizeNativeWindow}
        />
      </header>
    );
  }

  if (platform === "windows") {
    return (
      <header
        aria-label="Window drag region"
        className="welcome-screen__window-chrome fixed inset-x-0 top-0 z-20 grid h-10 grid-cols-[minmax(0,1fr)_auto] select-none items-center bg-(--bg-chrome) [-webkit-user-select:none]"
        data-tauri-drag-region
      >
        <span
          className="px-3 text-[12px] leading-none font-[620] text-(--text-heading)"
          data-tauri-drag-region
        >
          QingYu
        </span>
        <WindowsWindowControls
          onClose={closeNativeWindow}
          onMaximize={toggleNativeWindowMaximized}
          onMinimize={minimizeNativeWindow}
        />
      </header>
    );
  }

  return null;
}

export type DesktopStartupWorkspaceAuthoritativeStatus =
  | "unselected"
  | "invalid"
  | "unavailable"
  | "resolving"
  | "unsupported-version"
  | "starting"
  | "ready"
  | "failed";

export type DesktopStartupWorkspaceState =
  | { readonly status: "unselected" }
  | { readonly status: "selecting" }
  | { readonly status: "resolving" }
  | { readonly status: "upgrade-required" }
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
            publish(stateAfterStartupRequestFailure(startupStatus));
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
  if (status === "resolving") return { status: "resolving" };
  if (status === "unsupported-version") return { status: "upgrade-required" };
  return { status: "starting", workspacePath: null };
}

function stateAfterStartupRequestFailure(
  status: DesktopStartupWorkspaceAuthoritativeStatus
): DesktopStartupWorkspaceState {
  if (status === "unselected" || status === "invalid") {
    return { failure: "selection", status: "failed", workspacePath: null };
  }
  return stateFromAuthoritativeStatus(status);
}

export function DesktopStartupWorkspace(
  props: DesktopStartupWorkspaceProps
) {
  const controllerRef = useRef<DesktopStartupWorkspaceController | null>(null);
  if (controllerRef.current === null) {
    controllerRef.current = createDesktopStartupWorkspaceController(props);
  }
  const controller = controllerRef.current;
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot
  );
  useLayoutEffect(() => {
    controller.updateOptions(props);
    controller.updateStartupStatus(props.startupStatus);
  }, [
    controller,
    props.retryWorkspace,
    props.selectWorkspace,
    props.startWorkspace,
    props.startupStatus
  ]);
  useLayoutEffect(() => () => controller.cancelPending(), [controller]);
  const selecting = state.status === "selecting";
  const resolving = state.status === "resolving";
  const starting = state.status === "starting";
  const retrying = state.status === "retrying";
  const busy = resolving || starting || retrying;
  const failed = state.status === "failed";
  const upgradeRequired = state.status === "upgrade-required";
  const retryable = failed && state.failure === "startup";
  const selectionFailed = failed && !retryable;
  const platform = props.platform === undefined
    ? resolveDesktopStartupPlatform()
    : props.platform;

  return (
    <main
      className="welcome-screen welcome-screen--desktop"
      data-desktop-startup-workspace={state.status}
    >
      <DesktopStartupWindowChrome platform={platform} />
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
              {upgradeRequired
                ? "Update QingYu to continue"
                : busy
                  ? "Preparing QingYu"
                  : failed
                    ? selectionFailed
                      ? "QingYu could not choose a workspace"
                      : "QingYu could not open this workspace"
                    : "Choose your workspace"}
            </h1>
            <p role={failed || upgradeRequired ? "alert" : undefined}>
              {upgradeRequired
                ? "This native Kernel version is not supported. Update QingYu before opening the workspace."
                : busy
                  ? resolving
                    ? "QingYu is resolving the workspace saved by the native host."
                    : retrying
                      ? "QingYu is retrying the native Kernel without changing your workspace."
                      : "The native Kernel is opening your workspace."
                  : failed
                    ? selectionFailed
                      ? "The directory selector could not open. Try choosing the directory again."
                      : "The native Kernel could not start for this workspace. Retry when you are ready."
                    : "Select the local notebook directory QingYu should open."}
            </p>
          </div>
          {upgradeRequired ? null : busy ? (
            <div className="welcome-screen__status" role="status">
              <span
                aria-hidden="true"
                className="welcome-screen__spinner h-4 w-4 rounded-full border-2 border-(--border-strong) border-t-(--accent)"
              />
              <span>
                {resolving
                  ? "Resolving your workspace…"
                  : retrying
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
