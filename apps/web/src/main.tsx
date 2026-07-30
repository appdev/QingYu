import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App, { AppErrorBoundary, configureAppRuntime } from "@markra/app";
import { createKernelClient } from "@markra/kernel-client";
import "@markra/app/styles.css";
import { createServerWebRuntime } from "./runtime";
import { createServerKernelDomainAdapter } from "./runtime/server/kernel";
import {
  ServerStartupShell,
  startServerWebApplication,
} from "./server-application";
import { createServerWebBootstrapOwner } from "./server-bootstrap";
import { readServerCsrfCookie } from "./runtime/server/csrf";

const rootElement = document.getElementById("root");
if (rootElement === null) throw new Error("Web application root is unavailable.");
const root = createRoot(rootElement);
const browserOrigin = new URL("/", window.location.origin);
const client = createKernelClient({
  auth: {
    browserOrigin,
    getCsrfToken: () => readServerCsrfCookie(document.cookie),
    kind: "browser-session",
  },
  baseUrl: browserOrigin,
  fetch: window.fetch.bind(window),
});
const owner = createServerWebBootstrapOwner({
  client,
  createDomainAdapter: createServerKernelDomainAdapter,
});

const stop = startServerWebApplication({
  configureRuntime: configureAppRuntime,
  createRuntime: createServerWebRuntime,
  owner,
  renderApp: () => root.render(
    <StrictMode>
      <AppErrorBoundary>
        <App />
      </AppErrorBoundary>
    </StrictMode>,
  ),
  renderStartup: (snapshot, bootstrapOwner) => root.render(
    <StrictMode>
      <AppErrorBoundary>
        <ServerStartupShell owner={bootstrapOwner} snapshot={snapshot} />
      </AppErrorBoundary>
    </StrictMode>,
  ),
});

window.addEventListener("pagehide", stop, { once: true });
