import type {
  KernelDomainPort,
  KernelRuntimeSnapshot,
  KernelWorkspaceSnapshot,
} from "@markra/app/runtime";
import {
  KernelApiError,
  type KernelApiErrorDetails,
  type KernelClient,
} from "@markra/kernel-client";

type RuntimeSource = Awaited<ReturnType<KernelClient["system"]["runtime"]>>;
type WorkspaceSource = Awaited<ReturnType<KernelClient["workspace"]["get"]>>;

export type ServerKernelDomainAdapter = {
  port: KernelDomainPort;
  release: () => undefined;
};

export type ServerKernelDomainAdapterOptions = {
  instanceId: string;
  onAuthenticationRequired: () => unknown;
  workspaceGeneration: string;
  workspaceId: string;
};

export type ServerWebPromptError =
  | { kind: "invalid-credentials" }
  | { kind: "rate-limited"; retryAfterSeconds: number }
  | { kind: "server-unavailable" };

export type ServerWebBootstrapResult = {
  kernel: KernelDomainPort;
  runtime: KernelRuntimeSnapshot;
  workspace: KernelWorkspaceSnapshot;
};

export type ServerWebBootstrapSnapshot =
  | { phase: "checking" }
  | { phase: "initialize"; error: ServerWebPromptError | null }
  | { phase: "login"; error: ServerWebPromptError | null }
  | { phase: "starting" }
  | { phase: "ready"; result: ServerWebBootstrapResult }
  | { phase: "failed"; error: { kind: "server-unavailable" } }
  | { phase: "closed" };

export type ServerWebBootstrapOwner = {
  close: () => undefined;
  getSnapshot: () => ServerWebBootstrapSnapshot;
  initialize: (input: {
    initializationToken: string;
    password: string;
  }) => Promise<undefined>;
  login: (input: { password: string }) => Promise<undefined>;
  retry: () => Promise<undefined>;
  start: () => Promise<undefined>;
  subscribe: (
    subscriber: (snapshot: ServerWebBootstrapSnapshot) => unknown,
  ) => () => undefined;
};

export type ServerWebDomainAdapterFactory = (
  client: KernelClient,
  options: ServerKernelDomainAdapterOptions,
) => Promise<ServerKernelDomainAdapter>;

export interface ServerWebBootstrapOwnerOptions {
  readonly client: KernelClient;
  readonly createDomainAdapter: ServerWebDomainAdapterFactory;
}

export function createServerWebBootstrapOwner({
  client,
  createDomainAdapter,
}: ServerWebBootstrapOwnerOptions): ServerWebBootstrapOwner {
  const subscribers = new Set<
    (snapshot: ServerWebBootstrapSnapshot) => unknown
  >();
  let snapshot: ServerWebBootstrapSnapshot = { phase: "checking" };
  let activeAdapter: ServerKernelDomainAdapter | undefined;
  let closed = false;
  let operationGeneration = 0;

  const publish = (next: ServerWebBootstrapSnapshot) => {
    if (closed && next.phase !== "closed") return undefined;
    snapshot = next;
    for (const subscriber of [...subscribers]) {
      try {
        subscriber(next);
      } catch {
        // One view subscriber cannot interrupt the security state owner.
      }
    }
    return undefined;
  };
  const releaseAdapter = () => {
    const adapter = activeAdapter;
    activeAdapter = undefined;
    try {
      adapter?.release();
    } catch {
      // Releasing an already failed transport remains best-effort.
    }
    return undefined;
  };
  const isCurrent = (generation: number) => (
    !closed && generation === operationGeneration
  );
  const returnToLogin = () => {
    if (closed) return undefined;
    operationGeneration += 1;
    releaseAdapter();
    publish({ phase: "login", error: null });
    return undefined;
  };
  const fail = (generation: number) => {
    if (!isCurrent(generation)) return undefined;
    releaseAdapter();
    publish({ phase: "failed", error: { kind: "server-unavailable" } });
    return undefined;
  };

  const bootstrapAuthenticatedRuntime = async (generation: number) => {
    if (!isCurrent(generation)) return undefined;
    publish({ phase: "starting" });
    try {
      const ready = await client.system.ready();
      if (!isCurrent(generation)) return undefined;
      const runtime = await client.system.runtime();
      if (!isCurrent(generation)) return undefined;
      const workspace = await client.workspace.get();
      if (!isCurrent(generation)) return undefined;
      if (
        ready.instanceId !== runtime.instanceId ||
        !isReadyServerRuntime(runtime) ||
        !isReadyWorkspace(workspace)
      ) {
        return fail(generation);
      }

      const adapter = await createDomainAdapter(client, {
        instanceId: runtime.instanceId,
        onAuthenticationRequired: returnToLogin,
        workspaceGeneration: workspace.generation,
        workspaceId: workspace.id,
      });
      if (!isCurrent(generation)) {
        adapter.release();
        return undefined;
      }
      releaseAdapter();
      activeAdapter = adapter;
      publish({
        phase: "ready",
        result: {
          kernel: adapter.port,
          runtime: mapRuntime(runtime),
          workspace: mapWorkspace(workspace),
        },
      });
    } catch (error: unknown) {
      if (!isCurrent(generation)) return undefined;
      if (isUnauthorized(error)) {
        returnToLogin();
        return undefined;
      }
      fail(generation);
    }
    return undefined;
  };

  const start = async () => {
    if (closed) return undefined;
    const generation = ++operationGeneration;
    releaseAdapter();
    publish({ phase: "checking" });
    try {
      const status = await client.auth.status();
      if (!isCurrent(generation)) return undefined;
      if (status.initialization === "required") {
        publish({ phase: "initialize", error: null });
        return undefined;
      }
      if (status.initialization !== "initialized") return fail(generation);
      try {
        await client.auth.getSession();
      } catch (error: unknown) {
        if (!isCurrent(generation)) return undefined;
        if (isUnauthorized(error)) {
          publish({ phase: "login", error: null });
          return undefined;
        }
        return fail(generation);
      }
      return bootstrapAuthenticatedRuntime(generation);
    } catch {
      return fail(generation);
    }
  };

  const initialize = async (input: {
    initializationToken: string;
    password: string;
  }) => {
    if (closed || snapshot.phase !== "initialize") return undefined;
    const generation = ++operationGeneration;
    publish({ phase: "starting" });
    try {
      await client.auth.initialize({
        initializationToken: input.initializationToken,
        password: input.password,
      });
      if (!isCurrent(generation)) return undefined;
      return bootstrapAuthenticatedRuntime(generation);
    } catch (error: unknown) {
      if (!isCurrent(generation)) return undefined;
      if (isAlreadyInitialized(error)) {
        publish({ phase: "login", error: null });
        return undefined;
      }
      publish({
        phase: "initialize",
        error: promptError(error),
      });
      return undefined;
    }
  };

  const login = async (input: { password: string }) => {
    if (closed || snapshot.phase !== "login") return undefined;
    const generation = ++operationGeneration;
    publish({ phase: "starting" });
    try {
      await client.auth.login({ password: input.password });
      if (!isCurrent(generation)) return undefined;
      return bootstrapAuthenticatedRuntime(generation);
    } catch (error: unknown) {
      if (!isCurrent(generation)) return undefined;
      if (isInitializationRequired(error)) {
        publish({ phase: "initialize", error: null });
        return undefined;
      }
      publish({ phase: "login", error: promptError(error) });
      return undefined;
    }
  };

  return {
    close: () => {
      if (closed) return undefined;
      closed = true;
      operationGeneration += 1;
      releaseAdapter();
      publish({ phase: "closed" });
      subscribers.clear();
      return undefined;
    },
    getSnapshot: () => snapshot,
    initialize,
    login,
    retry: start,
    start,
    subscribe: (subscriber) => {
      if (closed) return () => undefined;
      subscribers.add(subscriber);
      subscriber(snapshot);
      return () => {
        subscribers.delete(subscriber);
        return undefined;
      };
    },
  };
}

function isReadyServerRuntime(runtime: RuntimeSource) {
  return runtime.profile === "server" &&
    runtime.startupState === "ready" &&
    runtime.instanceId.length > 0 &&
    runtime.capabilities.documents === true;
}

function isReadyWorkspace(workspace: WorkspaceSource) {
  return workspace.readiness === "ready" &&
    workspace.id.length > 0 &&
    workspace.generation.length > 0;
}

function mapRuntime(runtime: RuntimeSource): KernelRuntimeSnapshot {
  return {
    capabilities: {
      documents: runtime.capabilities.documents,
      history: runtime.capabilities.history,
      portableSettings: runtime.capabilities.portableSettings,
      resources: runtime.capabilities.resources,
      s3: runtime.capabilities.s3,
      search: runtime.capabilities.search,
      settings: runtime.capabilities.settings,
      sync: runtime.capabilities.sync,
      webdav: runtime.capabilities.webdav,
    },
    instanceId: runtime.instanceId,
    profile: runtime.profile,
    startupState: runtime.startupState,
  };
}

function mapWorkspace(workspace: WorkspaceSource): KernelWorkspaceSnapshot {
  return {
    displayName: workspace.displayName,
    generation: workspace.generation as KernelWorkspaceSnapshot["generation"],
    id: workspace.id,
    readiness: workspace.readiness,
    revision: workspace.revision as KernelWorkspaceSnapshot["revision"],
  };
}

function promptError(error: unknown): ServerWebPromptError {
  if (error instanceof KernelApiError) {
    if (error.code === "invalid_credentials") return { kind: "invalid-credentials" };
    if (error.code === "authentication_rate_limited") {
      const retryAfterSeconds = rateLimitSeconds(error.details);
      if (retryAfterSeconds !== null) {
        return { kind: "rate-limited", retryAfterSeconds };
      }
    }
  }
  return { kind: "server-unavailable" };
}

function rateLimitSeconds(details: KernelApiErrorDetails | undefined) {
  return details?.type === "rate-limit" ? details.retryAfterSeconds : null;
}

function isUnauthorized(error: unknown) {
  return error instanceof KernelApiError && error.code === "unauthorized";
}

function isAlreadyInitialized(error: unknown) {
  return error instanceof KernelApiError && error.code === "already_initialized";
}

function isInitializationRequired(error: unknown) {
  return error instanceof KernelApiError && error.code === "initialization_required";
}
