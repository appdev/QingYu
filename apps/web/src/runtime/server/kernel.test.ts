import type {
  KernelClient,
  KernelEventHandlers,
  KernelEventsClient,
} from "@markra/kernel-client";
import { KernelApiError, KernelEventError } from "@markra/kernel-client";
import type {
  KernelDocumentLocator,
  KernelHistorySnapshotId,
  KernelRevision,
  KernelWorkspaceGeneration,
} from "@markra/app/runtime";

import {
  createServerKernelDomainAdapter,
  ServerKernelDomainAdapterError,
} from "./kernel";

const INSTANCE_ID = "123e4567-e89b-42d3-a456-426614174000";
const GENERATION = "workspace-generation-1" as KernelWorkspaceGeneration;

describe("Server Kernel domain adapter", () => {
  it("maps the validated server runtime, fixed workspace, and paged documents", async () => {
    const client = kernelClient();
    const adapter = await createServerKernelDomainAdapter(client, options());

    await expect(adapter.port.runtime.read()).resolves.toMatchObject({
      instanceId: INSTANCE_ID,
      profile: "server",
      startupState: "ready",
    });
    await expect(adapter.port.workspace.read()).resolves.toMatchObject({
      id: "workspace-1",
      generation: GENERATION,
      readiness: "ready",
    });
    await expect(adapter.port.documents.list({
      workspaceGeneration: GENERATION,
    })).resolves.toEqual({
      items: [{
        kind: "file",
        locator: "signed-document-1",
        modifiedAt: "2026-07-30T00:00:00Z",
        name: "note.md",
        parent: "",
        relativePath: "note.md",
        revision: "revision-1",
        sizeBytes: 5,
        workspaceGeneration: GENERATION,
      }],
      nextCursor: null,
      workspaceGeneration: GENERATION,
    });
    expect(client.documents.list).toHaveBeenCalledWith({
      cursor: undefined,
      limit: undefined,
      parent: undefined,
    }, { signal: expect.any(AbortSignal) });

    await expect(adapter.port.documents.history.read({
      locator: "signed-document-1" as KernelDocumentLocator,
      snapshotId: "snapshot-1" as KernelHistorySnapshotId,
      workspaceGeneration: GENERATION,
    })).resolves.toEqual({
      contents: "older",
      documentLocator: "signed-document-1",
      revision: "revision-history-1",
      snapshotId: "snapshot-1",
      workspaceGeneration: GENERATION,
    });
  });

  it("rejects a caller generation mismatch before issuing a document request", async () => {
    const client = kernelClient();
    const adapter = await createServerKernelDomainAdapter(client, options());

    await expect(adapter.port.documents.list({
      workspaceGeneration: "stale-generation" as KernelWorkspaceGeneration,
    })).rejects.toMatchObject({ code: "workspace-generation-mismatch" });
    expect(client.documents.list).not.toHaveBeenCalled();
  });

  it("fails closed and requests login once when a later request is unauthorized", async () => {
    const onAuthenticationRequired = vi.fn();
    const client = kernelClient({
      runtimeError: new KernelApiError({
        code: "unauthorized",
        status: 401,
        requestId: "223e4567-e89b-42d3-a456-426614174000",
      }),
    });
    const adapter = await createServerKernelDomainAdapter(client, {
      ...options(),
      onAuthenticationRequired,
    });

    await expect(adapter.port.runtime.read()).rejects.toMatchObject({ code: "authentication-required" });
    expect(onAuthenticationRequired).toHaveBeenCalledOnce();
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
    expect(onAuthenticationRequired).toHaveBeenCalledOnce();
  });

  it("fails closed when the fixed server workspace identity drifts", async () => {
    const client = kernelClient({ workspaceId: "replacement-workspace" });
    const adapter = await createServerKernelDomainAdapter(client, options());

    await expect(adapter.port.workspace.read()).rejects.toBeInstanceOf(ServerKernelDomainAdapterError);
    await expect(adapter.port.runtime.read()).rejects.toMatchObject({ code: "released" });
  });

  it("owns the authenticated event connection and publishes validated Kernel events", async () => {
    let handlers: KernelEventHandlers | undefined;
    const close = vi.fn();
    const events = {
      connect: vi.fn((nextHandlers: KernelEventHandlers) => {
        handlers = nextHandlers;
        return { close, state: "connecting" as const };
      }),
    } satisfies KernelEventsClient;
    const adapter = await createServerKernelDomainAdapter(kernelClient(), {
      ...options(),
      events,
    });
    const listener = vi.fn();
    const unsubscribe = adapter.port.serverEvents.subscribe(listener);
    const frame = {
      connectionId: "223e4567-e89b-42d3-a456-426614174000",
      event: {
        document: {
          id: "signed-document-1",
          kind: "file",
          modifiedAt: "2026-07-30T00:00:01Z",
          name: "note.md",
          parent: "",
          path: "note.md",
          revision: "revision-2",
          sizeBytes: 6,
        },
        type: "document-changed",
      },
      protocolVersion: 1,
      resource: { id: "signed-document-1", kind: "document" },
      revision: "revision-2",
      sequence: 1,
      type: "event",
    } as const;

    handlers?.onEvent?.(frame);
    expect(listener).toHaveBeenCalledWith({ frame, kind: "event" });
    handlers?.onSnapshotRequired?.({
      reason: "sequence-gap",
      reloadScopes: ["documents", "workspace"],
    });
    expect(listener).toHaveBeenLastCalledWith({
      kind: "snapshot-required",
      reason: "sequence-gap",
      reloadScopes: ["documents", "workspace"],
    });
    unsubscribe();
    adapter.release();
    expect(close).toHaveBeenCalledOnce();
  });

  it("returns to authentication when the event stream rejects the browser session", async () => {
    let handlers: KernelEventHandlers | undefined;
    const close = vi.fn();
    const onAuthenticationRequired = vi.fn();
    const events = {
      connect: vi.fn((nextHandlers: KernelEventHandlers) => {
        handlers = nextHandlers;
        return { close, state: "open" as const };
      }),
    } satisfies KernelEventsClient;
    const adapter = await createServerKernelDomainAdapter(kernelClient(), {
      ...options(),
      events,
      onAuthenticationRequired,
    });

    handlers?.onError?.(new KernelEventError("server-error", {
      frameCode: "unauthorized",
    }));
    handlers?.onError?.(new KernelEventError("server-error", {
      frameCode: "unauthorized",
    }));

    expect(onAuthenticationRequired).toHaveBeenCalledOnce();
    expect(close).toHaveBeenCalledOnce();
    expect(adapter.port.serverEvents.available).toBe(false);
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
  });

  it("returns to startup authentication when the event stream reports a new Kernel instance", async () => {
    let handlers: KernelEventHandlers | undefined;
    const close = vi.fn();
    const onAuthenticationRequired = vi.fn();
    const events = {
      connect: vi.fn((nextHandlers: KernelEventHandlers) => {
        handlers = nextHandlers;
        return { close, state: "connecting" as const };
      }),
    } satisfies KernelEventsClient;
    const adapter = await createServerKernelDomainAdapter(kernelClient(), {
      ...options(),
      events,
      onAuthenticationRequired,
    });

    handlers?.onReady?.({
      connectionId: "223e4567-e89b-42d3-a456-426614174000",
      instanceId: "323e4567-e89b-42d3-a456-426614174000",
      protocolVersion: 1,
      sequence: 0,
      snapshotRequired: true,
      type: "ready",
    });

    expect(onAuthenticationRequired).toHaveBeenCalledOnce();
    expect(close).toHaveBeenCalledOnce();
    expect(adapter.port.serverEvents.available).toBe(false);
  });
});

function options() {
  return {
    instanceId: INSTANCE_ID,
    onAuthenticationRequired: vi.fn(),
    workspaceGeneration: GENERATION,
    workspaceId: "workspace-1",
  };
}

function kernelClient(overrides: {
  runtimeError?: unknown;
  workspaceId?: string;
} = {}) {
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
  };
  const workspace = {
    displayName: "Notes",
    generation: GENERATION,
    id: overrides.workspaceId ?? "workspace-1",
    readiness: "ready" as const,
    revision: "workspace-revision-1",
  };
  const runtimeRead = overrides.runtimeError === undefined
    ? vi.fn(async () => runtime)
    : vi.fn(async () => Promise.reject(overrides.runtimeError));

  return {
    auth: {},
    system: { runtime: runtimeRead },
    workspace: {
      get: vi.fn(async () => workspace),
      search: vi.fn(),
    },
    documents: {
      list: vi.fn(async () => ({
        items: [{
          id: "signed-document-1",
          kind: "file" as const,
          modifiedAt: "2026-07-30T00:00:00Z",
          name: "note.md",
          parent: "",
          path: "note.md",
          revision: "revision-1" as KernelRevision,
          sizeBytes: 5,
        }],
        nextCursor: null,
      })),
      create: vi.fn(),
      delete: vi.fn(),
      get: vi.fn(),
      getHistory: vi.fn(async () => ({
        contents: "older",
        documentId: "signed-document-1",
        revision: "revision-history-1",
        snapshotId: "snapshot-1",
      })),
      listHistory: vi.fn(),
      move: vi.fn(),
      restoreHistory: vi.fn(),
      update: vi.fn(),
    },
    settings: { get: vi.fn(), patch: vi.fn() },
    sync: {
      getConfig: vi.fn(),
      getStatus: vi.fn(),
      patchConfig: vi.fn(),
      testConnection: vi.fn(),
      trigger: vi.fn(),
    },
  } as unknown as KernelClient;
}
