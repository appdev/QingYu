import type { KernelClient } from "@markra/kernel-client";
import { KernelApiError } from "@markra/kernel-client";
import type { KernelDomainPort } from "@markra/app/runtime";

import {
  createServerWebBootstrapOwner,
  type ServerWebBootstrapSnapshot,
} from "./server-bootstrap";

const INSTANCE_ID = "123e4567-e89b-42d3-a456-426614174000";
const WORKSPACE_GENERATION = "workspace-generation-1";

describe("server Web bootstrap owner", () => {
  it("requires owner initialization without retaining submitted secrets", async () => {
    const client = bootstrapClient({ initialization: "required" });
    const owner = createServerWebBootstrapOwner({
      client,
      createDomainAdapter: domainAdapterFactory(),
    });

    await owner.start();
    expect(owner.getSnapshot()).toEqual({ phase: "initialize", error: null });

    const initializationToken = "initialization-token-must-not-survive";
    const password = "password-must-not-survive";
    await owner.initialize({ initializationToken, password });

    expect(client.auth.initialize).toHaveBeenCalledWith(
      { initializationToken, password },
      { signal: expect.any(AbortSignal) },
    );
    const rendered = JSON.stringify(owner.getSnapshot());
    expect(rendered).not.toContain(initializationToken);
    expect(rendered).not.toContain(password);
    expect(owner.getSnapshot().phase).toBe("ready");
  });

  it("uses an existing session or transitions an unauthorized initialized server to login", async () => {
    const authenticated = bootstrapClient({ initialization: "initialized" });
    const authenticatedOwner = createServerWebBootstrapOwner({
      client: authenticated,
      createDomainAdapter: domainAdapterFactory(),
    });
    await authenticatedOwner.start();
    expect(authenticatedOwner.getSnapshot().phase).toBe("ready");

    const signedOut = bootstrapClient({
      initialization: "initialized",
      sessionError: apiError("unauthorized", 401),
    });
    const signedOutOwner = createServerWebBootstrapOwner({
      client: signedOut,
      createDomainAdapter: domainAdapterFactory(),
    });
    await signedOutOwner.start();
    expect(signedOutOwner.getSnapshot()).toEqual({ phase: "login", error: null });
  });

  it("keeps invalid credentials and rate limits on a safe prompt without exposing causes", async () => {
    const invalidCredentials = bootstrapClient({
      initialization: "initialized",
      sessionError: apiError("unauthorized", 401),
      loginError: apiError("invalid_credentials", 401),
    });
    const invalidCredentialsOwner = createServerWebBootstrapOwner({
      client: invalidCredentials,
      createDomainAdapter: domainAdapterFactory(),
    });
    await invalidCredentialsOwner.start();
    await invalidCredentialsOwner.login({ password: "invalid-password-secret" });
    expect(invalidCredentialsOwner.getSnapshot()).toEqual({
      phase: "login",
      error: { kind: "invalid-credentials" },
    });

    const client = bootstrapClient({
      initialization: "initialized",
      sessionError: apiError("unauthorized", 401),
      loginError: apiError("authentication_rate_limited", 429, {
        type: "rate-limit",
        retryAfterSeconds: 31,
      }),
    });
    const owner = createServerWebBootstrapOwner({
      client,
      createDomainAdapter: domainAdapterFactory(),
    });
    await owner.start();
    await owner.login({ password: "rate-limited-password-secret" });

    expect(owner.getSnapshot()).toEqual({
      phase: "login",
      error: { kind: "rate-limited", retryAfterSeconds: 31 },
    });
    expect(JSON.stringify(owner.getSnapshot())).not.toContain("rate-limited-password-secret");
  });

  it("delivers a runtime only after exact ready, server profile, instance, and workspace checks", async () => {
    for (const override of [
      { ready: { instanceId: "223e4567-e89b-42d3-a456-426614174000" } },
      { runtime: { profile: "desktop" } },
      { runtime: { startupState: "starting" } },
      { runtime: { instanceId: "223e4567-e89b-42d3-a456-426614174000" } },
      { workspace: { readiness: "locked" } },
      { workspace: { generation: "" } },
    ] as const) {
      const client = bootstrapClient({ initialization: "initialized", ...override });
      const createDomainAdapter = domainAdapterFactory();
      const owner = createServerWebBootstrapOwner({ client, createDomainAdapter });
      await owner.start();

      expect(owner.getSnapshot()).toEqual({
        phase: "failed",
        error: { kind: "server-unavailable" },
      });
      expect(createDomainAdapter).not.toHaveBeenCalled();
    }
  });

  it("returns to login and releases the ready adapter when a later request loses authentication", async () => {
    let requestLogin: (() => unknown) | undefined;
    const release = vi.fn(() => undefined);
    const createDomainAdapter = vi.fn(async (
      _client: KernelClient,
      options: { onAuthenticationRequired: () => unknown },
    ) => {
      requestLogin = options.onAuthenticationRequired;
      return { port: {} as KernelDomainPort, release };
    });
    const owner = createServerWebBootstrapOwner({
      client: bootstrapClient({ initialization: "initialized" }),
      createDomainAdapter,
    });
    await owner.start();

    requestLogin?.();

    expect(release).toHaveBeenCalledOnce();
    expect(owner.getSnapshot()).toEqual({ phase: "login", error: null });
  });

  it("aborts stale bootstrap and secret-bearing requests on retry and close", async () => {
    const firstStatus = deferred<{ initialization: "required" }>();
    const client = bootstrapClient({ initialization: "required" });
    vi.mocked(client.auth.status)
      .mockImplementationOnce(async () => firstStatus.promise)
      .mockImplementationOnce(async () => ({ initialization: "required" }));
    const owner = createServerWebBootstrapOwner({
      client,
      createDomainAdapter: domainAdapterFactory(),
    });

    const firstStart = owner.start();
    const firstSignal = vi.mocked(client.auth.status).mock.calls[0]?.[0]?.signal;
    const retry = owner.retry();
    expect(firstSignal?.aborted).toBe(true);
    firstStatus.resolve({ initialization: "required" });
    await Promise.all([firstStart, retry]);

    const pendingInitialization = deferred<{ state: "authenticated" }>();
    vi.mocked(client.auth.initialize).mockImplementationOnce(
      async () => pendingInitialization.promise,
    );
    const initialization = owner.initialize({
      initializationToken: "secret-initialization-token",
      password: "secret-owner-password",
    });
    const initializationSignal = vi.mocked(client.auth.initialize).mock.calls[0]?.[1]?.signal;
    owner.close();
    expect(initializationSignal?.aborted).toBe(true);
    pendingInitialization.resolve({ state: "authenticated" });
    await initialization;
    expect(owner.getSnapshot()).toEqual({ phase: "closed" });
  });
});

function deferred<T>() {
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

function domainAdapterFactory() {
  return vi.fn(async () => ({
    port: {} as KernelDomainPort,
    release: vi.fn(() => undefined),
  }));
}

function bootstrapClient(options: {
  initialization: "required" | "initialized" | "unavailable";
  sessionError?: unknown;
  loginError?: unknown;
  ready?: Partial<{ instanceId: string }>;
  runtime?: Partial<{
    instanceId: string;
    profile: "desktop" | "mobile" | "server";
    startupState: "starting" | "ready";
  }>;
  workspace?: Partial<{ generation: string; readiness: "ready" | "locked" }>;
}) {
  const runtime = {
    capabilities: {
      documents: true,
      history: true,
      portableSettings: true,
      resources: true,
      s3: true,
      search: true,
      settings: true,
      sync: true,
      webdav: true,
    },
    instanceId: INSTANCE_ID,
    profile: "server" as const,
    startupState: "ready" as const,
    ...options.runtime,
  };
  const workspace = {
    displayName: "Notes",
    generation: WORKSPACE_GENERATION,
    id: "workspace-1",
    readiness: "ready" as const,
    revision: "revision-1",
    ...options.workspace,
  };
  const getSession = options.sessionError === undefined
    ? vi.fn(async () => ({ state: "authenticated" as const }))
    : vi.fn(async () => Promise.reject(options.sessionError));
  const login = options.loginError === undefined
    ? vi.fn(async () => ({ state: "authenticated" as const }))
    : vi.fn(async () => Promise.reject(options.loginError));

  return {
    auth: {
      status: vi.fn(async () => ({ initialization: options.initialization })),
      initialize: vi.fn(async () => ({ state: "authenticated" as const })),
      login,
      getSession,
      logout: vi.fn(),
      changePassword: vi.fn(),
    },
    system: {
      live: vi.fn(),
      ready: vi.fn(async () => ({
        apiVersion: "v1" as const,
        status: "ready" as const,
        instanceId: INSTANCE_ID,
        ...options.ready,
      })),
      version: vi.fn(),
      runtime: vi.fn(async () => runtime),
    },
    workspace: {
      get: vi.fn(async () => workspace),
      search: vi.fn(),
    },
    documents: {},
    resources: {},
    settings: {},
    sync: {},
  } as unknown as KernelClient;
}

function apiError(
  code: ConstructorParameters<typeof KernelApiError>[0]["code"],
  status: number,
  details?: ConstructorParameters<typeof KernelApiError>[0]["details"],
) {
  return new KernelApiError({
    code,
    status,
    requestId: "323e4567-e89b-42d3-a456-426614174000",
    details,
  });
}
