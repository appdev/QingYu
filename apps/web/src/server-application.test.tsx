import { renderToStaticMarkup } from "react-dom/server";
import type { AppRuntime, KernelDomainPort } from "@markra/app/runtime";

import {
  ServerStartupShell,
  startServerWebApplication,
} from "./server-application";
import type {
  ServerWebBootstrapOwner,
  ServerWebBootstrapSnapshot,
} from "./server-bootstrap";

describe("server Web application gate", () => {
  it("never configures or renders App before the authenticated runtime is ready", async () => {
    const snapshots: ServerWebBootstrapSnapshot[] = [{ phase: "checking" }];
    const subscribers = new Set<(snapshot: ServerWebBootstrapSnapshot) => unknown>();
    const owner = {
      start: vi.fn(async () => undefined),
      subscribe: vi.fn((subscriber: (snapshot: ServerWebBootstrapSnapshot) => unknown) => {
        subscribers.add(subscriber);
        subscriber(snapshots.at(-1)!);
        return () => {
          subscribers.delete(subscriber);
          return undefined;
        };
      }),
      getSnapshot: () => snapshots.at(-1)!,
      close: vi.fn(() => undefined),
      initialize: vi.fn(async () => undefined),
      login: vi.fn(async () => undefined),
      retry: vi.fn(async () => undefined),
    } satisfies ServerWebBootstrapOwner;
    const configureRuntime = vi.fn();
    const renderApp = vi.fn();
    const renderStartup = vi.fn();
    const kernel = {} as KernelDomainPort;
    const runtime = { kernel } as AppRuntime;

    const stop = startServerWebApplication({
      configureRuntime,
      createRuntime: vi.fn(() => runtime),
      owner,
      renderApp,
      renderStartup,
    });
    await owner.start();

    expect(renderStartup).toHaveBeenCalledWith({ phase: "checking" }, owner);
    expect(configureRuntime).not.toHaveBeenCalled();
    expect(renderApp).not.toHaveBeenCalled();

    const ready: ServerWebBootstrapSnapshot = {
      phase: "ready",
      result: {
        kernel,
        runtime: {
          capabilities: {
            documents: true, history: true, portableSettings: true,
            resources: true, s3: true, search: true, settings: true,
            sync: true, webdav: true,
          },
          instanceId: "123e4567-e89b-42d3-a456-426614174000",
          profile: "server",
          startupState: "ready",
        },
        workspace: {
          displayName: "Notes",
          generation: "generation-1" as never,
          id: "workspace-1",
          readiness: "ready",
          revision: "revision-1" as never,
        },
      },
    };
    snapshots.push(ready);
    for (const subscriber of subscribers) subscriber(ready);

    expect(configureRuntime).toHaveBeenCalledWith(runtime);
    expect(renderApp).toHaveBeenCalledOnce();
    stop();
    expect(owner.close).toHaveBeenCalledOnce();
  });

  it("renders only fixed safe startup copy and never embeds credential values", () => {
    const owner = inertOwner();
    const initialization = renderToStaticMarkup(
      <ServerStartupShell
        owner={owner}
        snapshot={{ phase: "initialize", error: null }}
      />,
    );
    expect(initialization).toContain("Initialize QingYu Server");
    expect(initialization).toContain("type=\"password\"");
    expect(initialization).not.toContain("value=");

    const limited = renderToStaticMarkup(
      <ServerStartupShell
        owner={owner}
        snapshot={{
          phase: "login",
          error: { kind: "rate-limited", retryAfterSeconds: 31 },
        }}
      />,
    );
    expect(limited).toContain("31 seconds");
  });
});

function inertOwner(): ServerWebBootstrapOwner {
  return {
    close: () => undefined,
    getSnapshot: () => ({ phase: "checking" }),
    initialize: async () => undefined,
    login: async () => undefined,
    retry: async () => undefined,
    start: async () => undefined,
    subscribe: () => () => undefined,
  };
}
