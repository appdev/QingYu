import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App, { AppErrorBoundary, configureAppRuntime } from "@markra/app";
import "@markra/app/styles.css";
import "./server-application.css";
import { createServerWebRuntime } from "./runtime";
import { createServerKernelDomainAdapter } from "./runtime/server/kernel";
import { createServerKernelTransports } from "./runtime/server/transports";
import {
  ServerStartupShell,
  startServerWebApplication,
} from "./server-application";
import { createServerWebBootstrapOwner } from "./server-bootstrap";
import { startServerStartupAppearance } from "./server-startup-appearance";
import { resolveServerStartupLanguage } from "./server-startup-language";

const rootElement = document.getElementById("root");
if (rootElement === null) throw new Error("Web application root is unavailable.");
const root = createRoot(rootElement);
const stopStartupAppearance = startServerStartupAppearance();
const startupLanguage = resolveServerStartupLanguage(
  window.location.search,
  navigator.languages,
);
document.documentElement.lang = startupLanguage;
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
  renderApp: () => {
    stopStartupAppearance();
    root.render(
      <StrictMode>
        <AppErrorBoundary>
          <App />
        </AppErrorBoundary>
      </StrictMode>,
    );
  },
  renderStartup: (snapshot, bootstrapOwner) => root.render(
    <StrictMode>
      <AppErrorBoundary>
        <ServerStartupShell
          language={startupLanguage}
          owner={bootstrapOwner}
          serverAddress={window.location.host}
          snapshot={snapshot}
          transport={window.location.protocol === "https:" ? "HTTPS" : "HTTP"}
        />
      </AppErrorBoundary>
    </StrictMode>,
  ),
});

window.addEventListener("pagehide", () => {
  stopStartupAppearance();
  stop();
}, { once: true });
