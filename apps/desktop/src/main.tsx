import { StrictMode, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import App, { AppErrorBoundary, configureAppRuntime } from "@markra/app";
import "@markra/app/styles.css";
import { bootstrapApplication, bootstrapApplicationMount } from "./bootstrap";
import type { DesktopStartupKernelSession } from "./desktop-application";
import {
  loadNativeRuntime,
  readNativeRuntimeKind,
} from "./runtime";

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

function KernelStartup({ session }: { session: DesktopStartupKernelSession }) {
  const status = session?.status ?? "checking";
  const message = status === "failed"
    ? "The native Kernel could not start. QingYu will retry without opening the workspace."
    : status === "retrying"
      ? "The native Kernel is restarting. Your workspace will open after it is ready."
      : "QingYu is starting the native Kernel and checking the workspace.";
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg-primary) px-6 py-8 text-(--text-primary)">
      <section className="w-full max-w-md rounded-md border border-(--border-default) bg-(--bg-primary) p-6">
        <h1 className="m-0 text-[20px] leading-7 font-bold text-(--text-heading)">
          Preparing QingYu
        </h1>
        <p className="m-0 mt-2 text-[13px] leading-5 text-(--text-secondary)">{message}</p>
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

async function startApplication() {
  const root = createRoot(document.getElementById("root")!);
  const reload = () => window.location.reload();

  if (readNativeRuntimeKind() === "mobile") {
    await bootstrapApplication({
      configureRuntime: configureAppRuntime,
      loadRuntime: loadNativeRuntime,
      reload,
      renderApp: () => renderRoot(root,
        <StrictMode>
          <AppErrorBoundary>
            <App />
          </AppErrorBoundary>
        </StrictMode>
      ),
      renderError: (onRetry) => renderError(root, onRetry)
    });
    return;
  }

  try {
    const { createProductionDesktopApplicationMountOwner } = await import(
      "./desktop-application-runtime"
    );
    const mountOwner = createProductionDesktopApplicationMountOwner({
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
      renderStartup: (session) => renderRoot(root,
        <StrictMode>
          <AppErrorBoundary>
            <KernelStartup session={session} />
          </AppErrorBoundary>
        </StrictMode>
      )
    });
    await bootstrapApplicationMount({
      mountOwner,
      reload,
      renderError: (onRetry) => renderError(root, onRetry)
    });
  } catch {
    renderError(root, reload);
  }
}

startApplication().catch(() => undefined);
