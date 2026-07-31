import { StrictMode, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import App, { AppErrorBoundary, configureAppRuntime } from "@markra/app";
import "@markra/app/styles.css";
import { bootstrapApplicationMount } from "./bootstrap";
import { createDesktopStartupApplicationOwner } from "./desktop-startup-application";
import {
  createDesktopKernelStartupOwner,
  initializeDesktopKernelWorkspace,
  retryDesktopKernelWorkspace,
  switchDesktopKernelWorkspace,
  type DesktopKernelStartupSnapshot,
} from "./desktop-kernel-startup";
import { DesktopStartupWorkspace } from "./desktop-startup-workspace";
import { selectDesktopWorkspaceDirectory } from "./desktop-workspace-selector";
import {
  readNativeRuntimeKind,
} from "./runtime";
import { retryMobileKernelRuntime } from "./runtime/mobile-kernel-session";

function StartupError({ onRetry }: { onRetry: () => unknown }) {
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg-primary) px-6 py-8 text-(--text-primary)">
      <section className="w-full max-w-md rounded-md border border-(--border-default) bg-(--bg-primary) p-6">
        <h1 className="m-0 text-[20px] leading-7 font-bold text-(--text-heading)">QingYu could not start</h1>
        <p className="m-0 mt-2 text-[13px] leading-5 text-(--text-secondary)">
          The native runtime failed to load. Retry after checking the application installation.
        </p>
        <button
          className="mt-5 inline-flex h-9 cursor-pointer items-center justify-center rounded-md border border-(--accent) bg-(--accent) px-4 text-[13px] font-[700] text-(--bg-primary)"
          type="button"
          onClick={onRetry}
        >
          Retry
        </button>
      </section>
    </main>
  );
}

function renderRoot(root: Root, node: ReactNode) {
  flushSync(() => root.render(node));
}

function renderError(root: Root, onRetry: () => unknown) {
  renderRoot(root,
    <StrictMode>
      <AppErrorBoundary>
        <StartupError onRetry={onRetry} />
      </AppErrorBoundary>
    </StrictMode>
  );
}

function renderDesktopStartupWorkspace(
  root: Root,
  snapshot: DesktopKernelStartupSnapshot,
) {
  renderRoot(root,
    <StrictMode>
      <AppErrorBoundary>
        <DesktopStartupWorkspace
          replaceWorkspace={switchDesktopKernelWorkspace}
          retryWorkspace={retryDesktopKernelWorkspace}
          selectWorkspace={selectDesktopWorkspaceDirectory}
          startWorkspace={initializeDesktopKernelWorkspace}
          startupStatus={snapshot.status}
        />
      </AppErrorBoundary>
    </StrictMode>
  );
}

function renderMobileKernelStartup(root: Root, status: string | null, onRetry: () => unknown) {
  if (status === "failed") {
    renderError(root, onRetry);
    return;
  }
  renderRoot(root,
    <StrictMode>
      <AppErrorBoundary>
        <main className="flex min-h-screen items-center justify-center bg-(--bg-primary) px-6 py-8 text-(--text-primary)">
          <section className="w-full max-w-md rounded-md border border-(--border-default) bg-(--bg-primary) p-6">
            <h1 className="m-0 text-[20px] leading-7 font-bold text-(--text-heading)">
              Starting QingYu
            </h1>
            <p className="m-0 mt-2 text-[13px] leading-5 text-(--text-secondary)">
              Preparing your private mobile workspace.
            </p>
          </section>
        </main>
      </AppErrorBoundary>
    </StrictMode>
  );
}

async function startApplication() {
  const root = createRoot(document.getElementById("root")!);
  const reload = () => window.location.reload();

  if (readNativeRuntimeKind() === "mobile") {
    const { createProductionMobileApplicationMountOwner } = await import(
      "./mobile-application-runtime"
    );
    const mountOwner = createProductionMobileApplicationMountOwner({
      configureRuntime: configureAppRuntime,
      renderDomain: () => {
        renderRoot(root,
          <StrictMode>
            <AppErrorBoundary>
              <App />
            </AppErrorBoundary>
          </StrictMode>
        );
        return () => renderRoot(root, null);
      },
      renderStartup: (session) => renderMobileKernelStartup(
        root,
        session?.status ?? null,
        () => retryMobileKernelRuntime(),
      ),
    });
    const stop = await bootstrapApplicationMount({
      mountOwner,
      reload,
      renderError: (onRetry) => renderError(root, onRetry),
    });
    window.addEventListener("pagehide", stop, { once: true });
    return;
  }

  try {
    const { createProductionDesktopApplicationMountOwner } = await import(
      "./desktop-application-runtime"
    );
    const startupOwner = createDesktopKernelStartupOwner();
    const renderWorkspace = (snapshot: DesktopKernelStartupSnapshot) =>
      renderDesktopStartupWorkspace(root, snapshot);
    const mountOwner = createDesktopStartupApplicationOwner({
      createApplicationMountOwner: () =>
        createProductionDesktopApplicationMountOwner({
          configureRuntime: configureAppRuntime,
          renderDomain: () => {
            renderRoot(root,
              <StrictMode>
                <AppErrorBoundary>
                  <App />
                </AppErrorBoundary>
              </StrictMode>
            );
            return () => renderRoot(root, null);
          },
          renderStartup: () => renderWorkspace(
            startupOwner.getSnapshot() ?? { status: "unavailable" },
          ),
        }),
      renderWorkspace,
      startupOwner,
    });
    const stop = await bootstrapApplicationMount({
      mountOwner,
      reload,
      renderError: (onRetry) => renderError(root, onRetry)
    });
    window.addEventListener("pagehide", stop, { once: true });
  } catch {
    renderError(root, reload);
  }
}

startApplication().catch(() => undefined);
