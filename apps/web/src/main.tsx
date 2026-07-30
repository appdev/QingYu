import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App, { AppErrorBoundary, configureAppRuntime } from "@markra/app";
import "@markra/app/styles.css";
import { createServerWebRuntime } from "./runtime";
import { createServerKernelDomainAdapter } from "./runtime/server/kernel";
import { createServerKernelTransports } from "./runtime/server/transports";
import {
  ServerStartupShell,
  startServerWebApplication,
} from "./server-application";
import { createServerWebBootstrapOwner } from "./server-bootstrap";

const rootElement = document.getElementById("root");
if (rootElement === null) throw new Error("Web application root is unavailable.");
const root = createRoot(rootElement);
const { client, events } = createServerKernelTransports({
  browserOrigin: window.location.origin,
  fetch: window.fetch.bind(window),
  readCookie: () => document.cookie,
  webSocket: (url) => new WebSocket(url),
});
const owner = createServerWebBootstrapOwner({
  client,
  createDomainAdapter: (domainClient, options) =>
    createServerKernelDomainAdapter(domainClient, { ...options, events }),
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
