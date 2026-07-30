import type { FormEvent } from "react";
import type { AppRuntime } from "@markra/app/runtime";

import type {
  ServerWebBootstrapOwner,
  ServerWebBootstrapSnapshot,
} from "./server-bootstrap";

export interface ServerStartupShellProps {
  readonly owner: ServerWebBootstrapOwner;
  readonly snapshot: ServerWebBootstrapSnapshot;
}

export function ServerStartupShell({
  owner,
  snapshot,
}: ServerStartupShellProps) {
  if (snapshot.phase === "initialize") {
    const submitInitialization = (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = event.currentTarget;
      const fields = new FormData(form);
      const initializationToken = readSecret(fields, "initializationToken");
      const password = readSecret(fields, "password");
      form.reset();
      owner.initialize({ initializationToken, password }).catch(() => undefined);
    };

    return (
      <main aria-labelledby="server-startup-title" className="server-startup">
        <form onSubmit={submitInitialization}>
          <h1 id="server-startup-title">Initialize QingYu Server</h1>
          <p>Set the password for this server before opening the workspace.</p>
          <label>
            Initialization token
            <input
              autoComplete="off"
              name="initializationToken"
              required
              type="password"
            />
          </label>
          <label>
            Password
            <input
              autoComplete="new-password"
              name="password"
              required
              type="password"
            />
          </label>
          <PromptError error={snapshot.error} />
          <button type="submit">Initialize</button>
        </form>
      </main>
    );
  }

  if (snapshot.phase === "login") {
    const submitLogin = (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = event.currentTarget;
      const password = readSecret(new FormData(form), "password");
      form.reset();
      owner.login({ password }).catch(() => undefined);
    };

    return (
      <main aria-labelledby="server-startup-title" className="server-startup">
        <form onSubmit={submitLogin}>
          <h1 id="server-startup-title">Sign in to QingYu Server</h1>
          <label>
            Password
            <input
              autoComplete="current-password"
              name="password"
              required
              type="password"
            />
          </label>
          <PromptError error={snapshot.error} />
          <button type="submit">Sign in</button>
        </form>
      </main>
    );
  }

  if (snapshot.phase === "failed") {
    return (
      <main aria-labelledby="server-startup-title" className="server-startup">
        <h1 id="server-startup-title">QingYu Server is unavailable</h1>
        <p>Check the server connection and try again.</p>
        <button
          onClick={() => owner.retry().catch(() => undefined)}
          type="button"
        >
          Try again
        </button>
      </main>
    );
  }

  if (snapshot.phase === "closed") return null;

  return (
    <main aria-busy="true" aria-live="polite" className="server-startup">
      {snapshot.phase === "checking"
        ? "Checking QingYu Server…"
        : "Starting QingYu Server…"}
    </main>
  );
}

export interface StartServerWebApplicationOptions {
  readonly configureRuntime: (runtime: AppRuntime) => unknown;
  readonly createRuntime: (
    kernel: Extract<ServerWebBootstrapSnapshot, { phase: "ready" }>["result"]["kernel"],
  ) => AppRuntime;
  readonly owner: ServerWebBootstrapOwner;
  readonly renderApp: () => unknown;
  readonly renderStartup: (
    snapshot: ServerWebBootstrapSnapshot,
    owner: ServerWebBootstrapOwner,
  ) => unknown;
}

export function startServerWebApplication({
  configureRuntime,
  createRuntime,
  owner,
  renderApp,
  renderStartup,
}: StartServerWebApplicationOptions) {
  let stopped = false;
  let mountedKernel: object | undefined;

  const unsubscribe = owner.subscribe((snapshot) => {
    if (stopped) return undefined;
    if (snapshot.phase !== "ready") {
      mountedKernel = undefined;
      renderStartup(snapshot, owner);
      return undefined;
    }
    if (mountedKernel === snapshot.result.kernel) return undefined;
    mountedKernel = snapshot.result.kernel;
    const runtime = createRuntime(snapshot.result.kernel);
    configureRuntime(runtime);
    renderApp();
    return undefined;
  });

  owner.start().catch(() => undefined);

  return () => {
    if (stopped) return undefined;
    stopped = true;
    unsubscribe();
    owner.close();
    return undefined;
  };
}

function PromptError({
  error,
}: {
  readonly error:
    | Extract<ServerWebBootstrapSnapshot, { phase: "initialize" | "login" }>["error"]
    | null;
}) {
  if (error === null) return null;
  if (error.kind === "rate-limited") {
    return (
      <p role="alert">
        Too many attempts. Try again in {error.retryAfterSeconds} seconds.
      </p>
    );
  }
  if (error.kind === "invalid-credentials") {
    return <p role="alert">The credentials were not accepted.</p>;
  }
  return <p role="alert">The server is unavailable. Try again.</p>;
}

function readSecret(form: FormData, name: string) {
  const value = form.get(name);
  return typeof value === "string" ? value : "";
}
