import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App, { AppErrorBoundary, configureAppRuntime } from "@markra/app";
import "@markra/app/styles.css";
import { bootstrapApplication } from "./bootstrap";
import { loadNativeRuntime } from "./runtime";

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

function startApplication() {
  const root = createRoot(document.getElementById("root")!);

  bootstrapApplication({
    configureRuntime: configureAppRuntime,
    loadRuntime: loadNativeRuntime,
    reload: () => window.location.reload(),
    renderApp: () => root.render(
      <StrictMode>
        <AppErrorBoundary>
          <App />
        </AppErrorBoundary>
      </StrictMode>
    ),
    renderError: (onRetry) => root.render(
      <StrictMode>
        <AppErrorBoundary>
          <StartupError onRetry={onRetry} />
        </AppErrorBoundary>
      </StrictMode>
    )
  });
}

startApplication();
