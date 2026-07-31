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
  KernelWorkspaceRelativePath,
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

  it("exhausts the authenticated inventory and retains only the fixed workspace identity", async () => {
    const client = kernelClient();
    vi.mocked(client.resources.list).mockImplementation(async (query) => (
      query?.cursor === undefined
        ? {
            items: [{
              entryType: "resource" as const,
              resource: {
                id: "image/payload.signature",
                kind: "image" as const,
                mediaType: "image/png",
                modifiedAt: "2026-07-30T00:00:00Z",
                name: "cover image.png",
                parent: "assets",
                path: "assets/cover image.png",
                previewable: true,
                revision: "resource-revision-1",
                sizeBytes: 7,
              },
            }],
            nextCursor: "inventory-page-2",
          }
        : { items: [], nextCursor: null }
    ));
    const adapter = await createServerKernelDomainAdapter(client, options());

    await expect(adapter.port.resources.list({
      parent: "assets" as KernelWorkspaceRelativePath,
      workspaceGeneration: GENERATION,
    })).resolves.toEqual({
      items: [{
        entryType: "resource",
        resource: {
          id: "image/payload.signature",
          kind: "image",
          mediaType: "image/png",
          modifiedAt: "2026-07-30T00:00:00Z",
          name: "cover image.png",
          parent: "assets",
          previewable: true,
          relativePath: "assets/cover image.png",
          revision: "resource-revision-1",
          sizeBytes: 7,
          workspaceGeneration: GENERATION,
        },
      }],
      workspaceGeneration: GENERATION,
    });
    expect(client.resources.list).toHaveBeenNthCalledWith(1, {
      cursor: undefined,
      limit: 100,
      parent: "assets",
    }, { signal: expect.any(AbortSignal) });
    expect(client.resources.list).toHaveBeenNthCalledWith(2, {
      cursor: "inventory-page-2",
      limit: 100,
      parent: "assets",
    }, { signal: expect.any(AbortSignal) });
  });

  it("writes raw resource blobs through the browser-session Kernel client", async () => {
    const client = kernelClient();
    vi.mocked(client.resources.create).mockResolvedValue({
      id: "resource.signature",
      kind: "attachment",
      mediaType: "application/octet-stream",
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "report.pdf",
      parent: "files",
      path: "files/report.pdf",
      previewable: false,
      revision: "sha256:resource-revision",
      sizeBytes: 7,
    });
    const adapter = await createServerKernelDomainAdapter(client, options());
    const body = new Blob(["report"], { type: "application/pdf" });

    await expect(adapter.port.resources.create({
      body,
      documentLocator: "signed-document-1" as KernelDocumentLocator,
      folder: "files" as KernelWorkspaceRelativePath,
      kind: "attachment",
      mediaType: "application/octet-stream",
      name: "report.pdf",
      workspaceGeneration: GENERATION,
    })).resolves.toMatchObject({
      id: "resource.signature",
      kind: "attachment",
      relativePath: "files/report.pdf",
      revision: "sha256:resource-revision",
      workspaceGeneration: GENERATION,
    });
    expect(client.resources.create).toHaveBeenCalledWith("signed-document-1", {
      body,
      folder: "files",
      kind: "attachment",
      mediaType: "application/octet-stream",
      name: "report.pdf",
      workspaceGeneration: GENERATION,
    }, { signal: expect.any(AbortSignal) });
  });

  it("fails closed when release races resource body consumption", async () => {
    const client = kernelClient();
    let resolveBody: ((body: Blob) => unknown) | undefined;
    let bodyStarted: (() => unknown) | undefined;
    const started = new Promise<undefined>((resolve) => {
      bodyStarted = () => resolve(undefined);
    });
    const response = new Response("pending", { headers: { "content-type": "image/png" } });
    Object.defineProperty(response, "blob", {
      value: () => {
        bodyStarted?.();
        return new Promise<Blob>((resolve) => {
          resolveBody = resolve;
        });
      },
    });
    vi.mocked(client.resources.open).mockResolvedValue(response);
    const adapter = await createServerKernelDomainAdapter(client, options());
    const opening = adapter.port.resources.open({
      id: "resource.signature",
      kind: "image",
      workspaceGeneration: GENERATION,
    });

    await started;
    adapter.release();
    resolveBody?.(new Blob(["retired"], { type: "image/png" }));

    await expect(opening).rejects.toMatchObject({ code: "released" });
  });

  it("fails closed when inventory authentication expires", async () => {
    const onAuthenticationRequired = vi.fn();
    const client = kernelClient();
    vi.mocked(client.resources.list).mockRejectedValue(new KernelApiError({
      code: "unauthorized",
      status: 401,
      requestId: "223e4567-e89b-42d3-a456-426614174000",
    }));
    const adapter = await createServerKernelDomainAdapter(client, {
      ...options(),
      onAuthenticationRequired,
    });

    await expect(adapter.port.resources.list({
      workspaceGeneration: GENERATION,
    })).rejects.toMatchObject({ code: "authentication-required" });
    expect(onAuthenticationRequired).toHaveBeenCalledOnce();
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
  });

  it("fails closed when an image resource request loses authentication", async () => {
    const onAuthenticationRequired = vi.fn();
    const client = kernelClient();
    vi.mocked(client.resources.open).mockRejectedValue(new KernelApiError({
      code: "unauthorized",
      status: 401,
      requestId: "223e4567-e89b-42d3-a456-426614174000",
    }));
    const adapter = await createServerKernelDomainAdapter(client, {
      ...options(),
      onAuthenticationRequired,
    });

    await expect(adapter.port.resources.open({
      id: "image/payload.signature",
      kind: "image",
      workspaceGeneration: GENERATION,
    })).rejects.toMatchObject({ code: "authentication-required" });
    expect(onAuthenticationRequired).toHaveBeenCalledOnce();
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
  });

  it("rejects a repeated inventory cursor instead of reusing an unbounded page", async () => {
    const client = kernelClient();
    vi.mocked(client.resources.list)
      .mockResolvedValueOnce({ items: [], nextCursor: "repeated-cursor" })
      .mockResolvedValueOnce({ items: [], nextCursor: "repeated-cursor" })
      .mockRejectedValueOnce(new Error("an unbounded third page must be unreachable"));
    const adapter = await createServerKernelDomainAdapter(client, options());

    await expect(adapter.port.resources.list({
      workspaceGeneration: GENERATION,
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(client.resources.list).toHaveBeenCalledTimes(2);
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
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
    const invalidationListener = vi.fn();
    const unsubscribe = adapter.port.serverEvents.subscribe(listener);
    const unsubscribeInvalidations = adapter.port.invalidations.subscribe(invalidationListener);
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
    expect(invalidationListener).toHaveBeenCalledWith({
      documentChange: "content",
      paths: ["note.md"],
      scopes: ["documents", "resources"],
    });
    handlers?.onSnapshotRequired?.({
      reason: "sequence-gap",
      reloadScopes: ["documents", "workspace"],
    });
    expect(listener).toHaveBeenLastCalledWith({
      kind: "snapshot-required",
      reason: "sequence-gap",
      reloadScopes: ["documents", "workspace"],
    });
    expect(invalidationListener).toHaveBeenLastCalledWith({
      documentChange: "snapshot",
      scopes: ["documents", "resources", "workspace"],
    });
    unsubscribe();
    unsubscribeInvalidations();
    adapter.release();
    expect(close).toHaveBeenCalledOnce();
  });

  it("maps every Kernel event family to the frozen invalidation scopes", async () => {
    let handlers: KernelEventHandlers | undefined;
    const events = {
      connect: vi.fn((nextHandlers: KernelEventHandlers) => {
        handlers = nextHandlers;
        return { close: vi.fn(), state: "open" as const };
      }),
    } satisfies KernelEventsClient;
    const adapter = await createServerKernelDomainAdapter(kernelClient(), {
      ...options(),
      events,
    });
    const invalidationListener = vi.fn();
    adapter.port.invalidations.subscribe(invalidationListener);
    const document = {
      id: "signed-document-1",
      kind: "file" as const,
      modifiedAt: "2026-07-30T00:00:01Z",
      name: "note.md",
      parent: "",
      path: "note.md",
      revision: "revision-2",
      sizeBytes: 6,
    };
    const cases = [
      [
        { type: "workspace-changed", workspace: { id: "workspace-1" } },
        { documentChange: "tree", scopes: ["workspace", "documents", "resources"] },
      ],
      [
        { document, type: "document-created" },
        { documentChange: "tree", paths: ["note.md"], scopes: ["documents", "resources"] },
      ],
      [
        { document, type: "document-changed" },
        { documentChange: "content", paths: ["note.md"], scopes: ["documents", "resources"] },
      ],
      [
        { document: { ...document, path: "archive/note.md" }, previousPath: "note.md", type: "document-moved" },
        {
          documentChange: "tree",
          paths: ["note.md", "archive/note.md"],
          scopes: ["documents", "resources"],
        },
      ],
      [
        { previousPath: "note.md", type: "document-deleted" },
        { documentChange: "tree", paths: ["note.md"], scopes: ["documents", "resources"] },
      ],
      [{ settings: {}, type: "settings-changed" }, { scopes: ["settings"] }],
      [{ config: {}, type: "sync-config-changed" }, { scopes: ["sync-config"] }],
      [
        { status: { completionState: "attempting" }, type: "sync-status-changed" },
        { scopes: ["sync-status"] },
      ],
      [
        { status: { completionState: "succeeded" }, type: "sync-status-changed" },
        {
          documentChange: "tree",
          scopes: ["sync-status", "documents", "resources"],
        },
      ],
    ] as const;

    cases.forEach(([event], index) => handlers?.onEvent?.({
      connectionId: "223e4567-e89b-42d3-a456-426614174000",
      event,
      protocolVersion: 1,
      resource: { kind: "workspace" },
      revision: `revision-${index + 1}`,
      sequence: index + 1,
      type: "event",
    } as never));

    expect(invalidationListener.mock.calls.map(([notice]) => notice))
      .toEqual(cases.map(([, expected]) => expected));
    handlers?.onSnapshotRequired?.({
      reason: "reconnect",
      reloadScopes: ["sync-status"],
    });
    expect(invalidationListener).toHaveBeenLastCalledWith({
      scopes: ["sync-status", "documents", "resources"],
    });
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
    resources: {
      create: vi.fn(),
      list: vi.fn(async () => ({ items: [], nextCursor: null })),
      open: vi.fn(async () => new Response(new Uint8Array([1]), {
        headers: { "content-type": "image/png" },
      })),
    },
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
