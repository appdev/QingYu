import type { FetchLike } from "@markra/kernel-client";
import type {
  KernelDocumentLocator,
  KernelRevision,
  KernelWorkspaceGeneration,
} from "@markra/app/runtime";
import type { NativeKernelBootstrap } from "../kernel-bootstrap";

import {
  createDesktopKernelDomainAdapter,
  type DesktopKernelConnection,
} from "./kernel";

const INSTANCE_ID = "123e4567-e89b-42d3-a456-426614174000";
const WORKSPACE_ID = "123e4567-e89b-42d3-a456-426614174001";
const REQUEST_ID = "123e4567-e89b-42d3-a456-426614174002";
const CREDENTIAL = "kernel-credential-must-remain-private";
const BASE_URL = "http://127.0.0.1:49152/";
const PROCESS_GENERATION = "7";
const WORKSPACE_GENERATION = "123e4567-e89b-42d3-a456-426614174003";

describe("desktop Kernel domain adapter", () => {
  it("accepts both the native bootstrap and the explicit connection contract", () => {
    expectTypeOf<NativeKernelBootstrap>().toMatchTypeOf<DesktopKernelConnection>();
    expectTypeOf<{
      authentication: DesktopKernelConnection["authentication"];
      baseUrl: string;
      instanceId: string;
      processGeneration: string;
    }>().toMatchTypeOf<DesktopKernelConnection>();
  });

  it("becomes available only after an authenticated ready/runtime/workspace handshake", async () => {
    const calls: Array<{ authorization: string | null; pathname: string }> = [];
    const fetch: FetchLike = async (url, init = {}) => {
      const pathname = new URL(url).pathname;
      calls.push({
        authorization: new Headers(init.headers).get("authorization"),
        pathname,
      });
      return handshakeResponse(pathname);
    };

    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });

    expect(adapter.port.availability).toBe("available");
    expect(calls).toEqual([
      { authorization: `Bearer ${CREDENTIAL}`, pathname: "/api/v1/health/ready" },
      { authorization: `Bearer ${CREDENTIAL}`, pathname: "/api/v1/runtime" },
      { authorization: `Bearer ${CREDENTIAL}`, pathname: "/api/v1/workspace" },
    ]);
  });

  it("releases ownership and returns a redacted error when initialization disagrees with bootstrap identity", async () => {
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/runtime") {
        return jsonResponse({
          ...runtimeBody(),
          instanceId: "123e4567-e89b-42d3-a456-426614174099",
        });
      }
      return handshakeResponse(pathname);
    };

    let thrown: unknown;
    try {
      await createDesktopKernelDomainAdapter(connection({ release }), { fetch });
    } catch (error: unknown) {
      thrown = error;
    }

    expect(thrown).toMatchObject({
      code: "initialization-failed",
      name: "DesktopKernelDomainAdapterError",
    });
    expect(String(thrown)).not.toContain(CREDENTIAL);
    expect(String(thrown)).not.toContain(BASE_URL);
    expect(release).toHaveBeenCalledOnce();
  });

  it.each([
    [
      "ready instance",
      "/api/v1/health/ready",
      { ...readyBody(), instanceId: "123e4567-e89b-42d3-a456-426614174099" },
    ],
    ["runtime profile", "/api/v1/runtime", { ...runtimeBody(), profile: "server" }],
    [
      "runtime startup state",
      "/api/v1/runtime",
      { ...runtimeBody(), startupState: "starting" },
    ],
    [
      "document capability",
      "/api/v1/runtime",
      { ...runtimeBody(), capabilities: { ...runtimeBody().capabilities, documents: false } },
    ],
    ["workspace readiness", "/api/v1/workspace", { ...workspaceBody(), readiness: "locked" }],
  ])("fails initialization when the %s handshake field differs", async (_name, path, body) => {
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      return pathname === path ? jsonResponse(body) : handshakeResponse(pathname);
    };

    await expect(
      createDesktopKernelDomainAdapter(connection({ release }), { fetch }),
    ).rejects.toMatchObject({
      code: "initialization-failed",
      name: "DesktopKernelDomainAdapterError",
    });
    expect(release).toHaveBeenCalledOnce();
  });

  it("redacts authentication provider failures while releasing the connection", async () => {
    const release = vi.fn(() => undefined);
    const fetch = vi.fn<FetchLike>();

    let thrown: unknown;
    try {
      await createDesktopKernelDomainAdapter(
        connection({
          authentication: {
            kind: "native-bearer",
            getCredential: () => {
              throw new Error(`provider leaked ${CREDENTIAL}`);
            },
          },
          release,
        }),
        { fetch },
      );
    } catch (error: unknown) {
      thrown = error;
    }

    expect(thrown).toMatchObject({ code: "initialization-failed" });
    expect(String(thrown)).not.toContain(CREDENTIAL);
    expect(fetch).not.toHaveBeenCalled();
    expect(release).toHaveBeenCalledOnce();
  });

  it("maps later runtime and workspace snapshots field by field", async () => {
    const fetch: FetchLike = async (url) => handshakeResponse(new URL(url).pathname);
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });

    await expect(adapter.port.runtime.read()).resolves.toEqual({
      capabilities: {
        documents: true,
        history: true,
        portableSettings: true,
        s3: false,
        search: true,
        settings: true,
        sync: true,
        webdav: true,
      },
      instanceId: INSTANCE_ID,
      profile: "desktop",
      startupState: "ready",
    });
    await expect(adapter.port.workspace.read()).resolves.toEqual({
      displayName: "Notes",
      generation: WORKSPACE_GENERATION,
      id: WORKSPACE_ID,
      readiness: "ready",
      revision: "workspace-revision-1",
    });
  });

  it("maps document reads and updates without exposing Kernel paths or transport details", async () => {
    const documentId = "document.signature";
    const requests: Array<{ body: unknown; method: string; pathname: string }> = [];
    const fetch: FetchLike = async (url, init = {}) => {
      const pathname = new URL(url).pathname;
      if (pathname.startsWith("/api/v1/documents/")) {
        const body = init.body === undefined ? undefined : JSON.parse(String(init.body));
        requests.push({ body, method: String(init.method), pathname });
        return jsonResponse({
          contents: init.method === "PUT" ? "updated" : "original",
          id: documentId,
          kind: "file",
          modifiedAt: "2026-07-30T00:00:00Z",
          name: "draft.md",
          parent: "private/notes",
          path: "private/notes/draft.md",
          revision: init.method === "PUT" ? "revision-2" : "revision-1",
          sizeBytes: init.method === "PUT" ? 7 : 8,
        });
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const locator = documentId as KernelDocumentLocator;
    const workspaceGeneration = WORKSPACE_GENERATION as KernelWorkspaceGeneration;

    const read = await adapter.port.documents.read({ locator, workspaceGeneration });
    const updated = await adapter.port.documents.update({
      contents: "updated",
      expectedRevision: "revision-1" as KernelRevision,
      locator,
      workspaceGeneration,
    });

    expect(read).toEqual({
      contents: "original",
      locator,
      modifiedAt: "2026-07-30T00:00:00Z",
      name: "draft.md",
      revision: "revision-1",
      sizeBytes: 8,
      workspaceGeneration,
    });
    expect(updated).toEqual({
      contents: "updated",
      locator,
      modifiedAt: "2026-07-30T00:00:00Z",
      name: "draft.md",
      revision: "revision-2",
      sizeBytes: 7,
      workspaceGeneration,
    });
    expect(requests).toEqual([
      {
        body: undefined,
        method: "GET",
        pathname: "/api/v1/documents/document.signature",
      },
      {
        body: {
          contents: "updated",
          expectedRevision: "revision-1",
          workspaceGeneration: WORKSPACE_GENERATION,
        },
        method: "PUT",
        pathname: "/api/v1/documents/document.signature",
      },
    ]);
    expect(JSON.stringify({ read, updated })).not.toContain("private/notes");
    expect(JSON.stringify({ read, updated })).not.toContain(BASE_URL);
    expect(JSON.stringify({ read, updated })).not.toContain(CREDENTIAL);
  });

  it("fails closed after a later protocol mismatch and never falls back to native workspace mutation", async () => {
    let runtimeReads = 0;
    let documentRequests = 0;
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/runtime") {
        runtimeReads += 1;
        if (runtimeReads > 1) {
          return jsonResponse({
            ...runtimeBody(),
            profile: "server",
          });
        }
      }
      if (pathname.startsWith("/api/v1/documents/")) documentRequests += 1;
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(adapter.port.runtime.read()).rejects.toMatchObject({
      code: "protocol-mismatch",
      name: "DesktopKernelDomainAdapterError",
    });
    await expect(
      adapter.port.documents.read({
        locator: "document.signature" as KernelDocumentLocator,
        workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ code: "released" });
    expect(documentRequests).toBe(0);
    expect(release).toHaveBeenCalledOnce();
  });

  it("releases a successful adapter idempotently and never asks for its credential again", async () => {
    const getCredential = vi.fn(() => CREDENTIAL);
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => handshakeResponse(new URL(url).pathname);
    const adapter = await createDesktopKernelDomainAdapter(
      connection({
        authentication: { kind: "native-bearer", getCredential },
        release,
      }),
      { fetch },
    );
    const credentialReadsAtRelease = getCredential.mock.calls.length;

    adapter.release();
    adapter.release();

    await expect(adapter.port.workspace.read()).rejects.toMatchObject({
      code: "released",
      name: "DesktopKernelDomainAdapterError",
    });
    expect(getCredential).toHaveBeenCalledTimes(credentialReadsAtRelease);
    expect(release).toHaveBeenCalledOnce();
  });

  it("rejects a stale workspace generation before issuing a document request", async () => {
    let documentRequests = 0;
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname.startsWith("/api/v1/documents/")) documentRequests += 1;
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });

    await expect(
      adapter.port.documents.read({
        locator: "document.signature" as KernelDocumentLocator,
        workspaceGeneration: "8" as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ code: "workspace-generation-mismatch" });
    expect(documentRequests).toBe(0);
  });

  it("closes the adapter when a document response changes the opaque identity", async () => {
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname.startsWith("/api/v1/documents/")) {
        return jsonResponse({
          contents: "original",
          id: "different.signature",
          kind: "file",
          modifiedAt: "2026-07-30T00:00:00Z",
          name: "draft.md",
          parent: "notes",
          path: "notes/draft.md",
          revision: "revision-1",
          sizeBytes: 8,
        });
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(
      adapter.port.documents.read({
        locator: "document.signature" as KernelDocumentLocator,
        workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ code: "protocol-mismatch" });
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
    expect(release).toHaveBeenCalledOnce();
  });
});

type ExplicitDesktopKernelConnection = Extract<
  DesktopKernelConnection,
  { processGeneration: string }
>;

function connection(
  overrides: Partial<ExplicitDesktopKernelConnection> = {},
): ExplicitDesktopKernelConnection {
  return {
    authentication: {
      kind: "native-bearer",
      getCredential: () => CREDENTIAL,
    },
    baseUrl: BASE_URL,
    processGeneration: PROCESS_GENERATION,
    instanceId: INSTANCE_ID,
    ...overrides,
  };
}

function handshakeResponse(pathname: string) {
  if (pathname === "/api/v1/health/ready") {
    return jsonResponse(readyBody());
  }
  if (pathname === "/api/v1/runtime") {
    return jsonResponse(runtimeBody());
  }
  if (pathname === "/api/v1/workspace") {
    return jsonResponse(workspaceBody());
  }
  throw new Error("unexpected request");
}

function readyBody() {
  return { apiVersion: "v1", instanceId: INSTANCE_ID, status: "ready" as const };
}

function runtimeBody() {
  return {
    capabilities: {
      documents: true,
      history: true,
      portableSettings: true,
      s3: false,
      search: true,
      settings: true,
      sync: true,
      webdav: true,
    },
    instanceId: INSTANCE_ID,
    profile: "desktop" as const,
    startupState: "ready" as const,
  };
}

function workspaceBody() {
  return {
    displayName: "Notes",
    generation: WORKSPACE_GENERATION,
    id: WORKSPACE_ID,
    readiness: "ready" as const,
    revision: "workspace-revision-1",
  };
}

function jsonResponse(body: unknown, status = 200) {
  return Response.json(body, {
    status,
    headers: { "x-request-id": REQUEST_ID },
  });
}
