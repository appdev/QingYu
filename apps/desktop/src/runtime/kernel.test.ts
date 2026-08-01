import type { FetchLike } from "@markra/kernel-client";
import type {
  KernelDocumentLocator,
  KernelDomainPort,
  KernelHistorySnapshotId,
  KernelPageCursor,
  KernelRevision,
  KernelWorkspaceGeneration,
  KernelWorkspaceRelativePath,
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
      { authorization: `Bearer ${CREDENTIAL}`, pathname: "/api/v1/app-config" },
    ]);
    expect(adapter.port.appConfig.bootstrap.workspace).toEqual({
      generation: WORKSPACE_GENERATION,
      id: WORKSPACE_ID,
    });
    expect(Object.isFrozen(adapter.port.appConfig.bootstrap.localState)).toBe(true);
  });

  it.each([
    ["workspace id", { workspaceId: "123e4567-e89b-42d3-a456-426614174099" }],
    ["workspace generation", { generation: "workspace-generation-2" }],
  ])("fails closed when app config has another %s", async (_field, appConfigOverrides) => {
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      return pathname === "/api/v1/app-config"
        ? jsonResponse(appConfigBody(appConfigOverrides))
        : handshakeResponse(pathname);
    };

    await expect(createDesktopKernelDomainAdapter(connection({ release }), { fetch }))
      .rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(release).toHaveBeenCalledOnce();
  });

  it("requires the explicitly selected mobile profile for an in-process mobile host", async () => {
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/runtime") {
        return jsonResponse({ ...runtimeBody(), profile: "mobile" });
      }
      return handshakeResponse(pathname);
    };

    const adapter = await createDesktopKernelDomainAdapter(connection(), {
      fetch,
      profile: "mobile",
    });

    await expect(adapter.port.runtime.read()).resolves.toMatchObject({ profile: "mobile" });
    adapter.release();
  });

  it("binds the default browser fetch to the global receiver", async () => {
    const receivers: unknown[] = [];
    const receiverSensitiveFetch: FetchLike = async function (
      this: unknown,
      url,
    ) {
      receivers.push(this);
      if (this !== globalThis) throw new TypeError("invalid fetch receiver");
      return handshakeResponse(new URL(url).pathname);
    };
    vi.stubGlobal("fetch", receiverSensitiveFetch);

    try {
      const adapter = await createDesktopKernelDomainAdapter(connection());

      expect(adapter.port.availability).toBe("available");
      expect(receivers).toEqual([globalThis, globalThis, globalThis, globalThis]);
    } finally {
      vi.unstubAllGlobals();
    }
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
        resources: true,
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
      kind: "file",
      locator,
      modifiedAt: "2026-07-30T00:00:00Z",
      name: "draft.md",
      parent: "private/notes",
      relativePath: "private/notes/draft.md",
      revision: "revision-1",
      sizeBytes: 8,
      workspaceGeneration,
    });
    expect(updated).toEqual({
      contents: "updated",
      kind: "file",
      locator,
      modifiedAt: "2026-07-30T00:00:00Z",
      name: "draft.md",
      parent: "private/notes",
      relativePath: "private/notes/draft.md",
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
    expect(JSON.stringify({ read, updated })).not.toContain(BASE_URL);
    expect(JSON.stringify({ read, updated })).not.toContain(CREDENTIAL);
  });

  it("maps paginated file and directory listings plus search results through web-safe fields", async () => {
    const requests: Array<{ pathname: string; query: Record<string, string> }> = [];
    const fetch: FetchLike = async (url) => {
      const parsed = new URL(url);
      if (parsed.pathname === "/api/v1/documents") {
        requests.push({ pathname: parsed.pathname, query: Object.fromEntries(parsed.searchParams) });
        return jsonResponse({
          items: [
            documentEntry("file.signature", "file"),
            documentEntry("directory.signature", "directory", {
              name: "archive",
              path: "notes/archive",
              sizeBytes: 0,
            }),
          ],
          nextCursor: "next.documents",
        });
      }
      if (parsed.pathname === "/api/v1/search") {
        requests.push({ pathname: parsed.pathname, query: Object.fromEntries(parsed.searchParams) });
        return jsonResponse({
          items: [{
            column: 3,
            document: documentEntry("file.signature", "file"),
            line: 2,
            preview: "a needle in a note",
          }],
          nextCursor: "next.search",
        });
      }
      return handshakeResponse(parsed.pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const workspaceGeneration = WORKSPACE_GENERATION as KernelWorkspaceGeneration;

    const listed = await adapter.port.documents.list({
      cursor: "cursor.documents" as KernelPageCursor,
      limit: 2,
      parent: "notes" as KernelWorkspaceRelativePath,
      workspaceGeneration,
    });
    const searched = await adapter.port.documents.search({
      cursor: "cursor.search" as KernelPageCursor,
      limit: 2,
      query: "needle",
      workspaceGeneration,
    });

    expect(listed).toEqual({
      items: [
        documentSnapshot("file.signature", "file", workspaceGeneration),
        documentSnapshot("directory.signature", "directory", workspaceGeneration, {
          name: "archive",
          relativePath: "notes/archive",
          sizeBytes: 0,
        }),
      ],
      nextCursor: "next.documents",
      workspaceGeneration,
    });
    expect(searched).toEqual({
      items: [{
        column: 3,
        document: documentSnapshot("file.signature", "file", workspaceGeneration),
        line: 2,
        preview: "a needle in a note",
      }],
      nextCursor: "next.search",
      workspaceGeneration,
    });
    expect(requests).toEqual([
      {
        pathname: "/api/v1/documents",
        query: { cursor: "cursor.documents", limit: "2", parent: "notes" },
      },
      {
        pathname: "/api/v1/search",
        query: { cursor: "cursor.search", limit: "2", query: "needle" },
      },
    ]);
    expect(JSON.stringify({ listed, searched })).not.toContain(BASE_URL);
    expect(JSON.stringify({ listed, searched })).not.toContain(CREDENTIAL);
    expect(JSON.stringify({ listed, searched })).not.toContain("absolutePath");
  });

  it("creates files and directories using the frozen generation-only create contract", async () => {
    const requests: unknown[] = [];
    const fetch: FetchLike = async (url, init = {}) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/documents" && init.method === "POST") {
        const body = JSON.parse(String(init.body));
        requests.push(body);
        if (body.kind === "file") {
          return jsonResponse({
            ...documentEntry("created.file", "file", {
              name: body.name,
              path: `notes/${body.name}`,
              sizeBytes: 7,
            }),
            contents: body.contents,
          }, 201);
        }
        return jsonResponse(documentEntry("created.directory", "directory", {
          name: body.name,
          path: `notes/${body.name}`,
          sizeBytes: 0,
        }), 201);
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const workspaceGeneration = WORKSPACE_GENERATION as KernelWorkspaceGeneration;
    const parent = "notes" as KernelWorkspaceRelativePath;

    await expect(adapter.port.documents.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent,
      workspaceGeneration,
    })).resolves.toMatchObject({
      contents: "created",
      kind: "file",
      locator: "created.file",
      relativePath: "notes/created.md",
      workspaceGeneration,
    });
    await expect(adapter.port.documents.create({
      kind: "directory",
      name: "archive",
      parent,
      workspaceGeneration,
    })).resolves.toMatchObject({
      kind: "directory",
      locator: "created.directory",
      relativePath: "notes/archive",
      workspaceGeneration,
    });
    expect(requests).toEqual([
      {
        contents: "created",
        kind: "file",
        name: "created.md",
        parent: "notes",
        workspaceGeneration: WORKSPACE_GENERATION,
      },
      {
        kind: "directory",
        name: "archive",
        parent: "notes",
        workspaceGeneration: WORKSPACE_GENERATION,
      },
    ]);
    expect(requests.every((request) => (
      typeof request === "object" &&
      request !== null &&
      !("processGeneration" in request)
    ))).toBe(true);
  });

  it("moves, deletes, lists history, and restores with explicit CAS request bodies", async () => {
    const requests: Array<{ body: unknown; method: string; pathname: string; search: string }> = [];
    const locator = "document.signature" as KernelDocumentLocator;
    const snapshotId = "123e4567-e89b-42d3-a456-426614174050" as KernelHistorySnapshotId;
    const fetch: FetchLike = async (url, init = {}) => {
      const parsed = new URL(url);
      if (parsed.pathname.includes("/api/v1/documents/document.signature")) {
        const body = init.body === undefined ? undefined : JSON.parse(String(init.body));
        requests.push({
          body,
          method: String(init.method),
          pathname: parsed.pathname,
          search: parsed.search,
        });
        if (parsed.pathname.endsWith("/move")) {
          return jsonResponse(documentEntry("moved.signature", "file", {
            name: "renamed.md",
            parent: "archive",
            path: "archive/renamed.md",
          }));
        }
        if (parsed.pathname.endsWith("/delete")) {
          return new Response(null, {
            headers: { "x-request-id": REQUEST_ID },
            status: 204,
          });
        }
        if (parsed.pathname.endsWith("/history")) {
          return jsonResponse({
            items: [{
              createdAt: "2026-07-29T23:59:00Z",
              documentId: "document.signature",
              revision: "revision-1",
              sizeBytes: 8,
              snapshotId,
            }],
            nextCursor: "next.history",
          });
        }
        if (parsed.pathname.endsWith(`/${snapshotId}/restore`)) {
          return jsonResponse({
            ...documentEntry("document.signature", "file"),
            contents: "restored",
          });
        }
      }
      return handshakeResponse(parsed.pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const workspaceGeneration = WORKSPACE_GENERATION as KernelWorkspaceGeneration;
    const expectedRevision = "revision-1" as KernelRevision;

    await expect(adapter.port.documents.move({
      expectedRevision,
      locator,
      name: "renamed.md",
      targetParent: "archive" as KernelWorkspaceRelativePath,
      workspaceGeneration,
    })).resolves.toMatchObject({
      locator: "moved.signature",
      name: "renamed.md",
      relativePath: "archive/renamed.md",
    });
    await expect(adapter.port.documents.delete({
      deletionPolicy: "recoverable",
      expectedRevision,
      locator,
      workspaceGeneration,
    })).resolves.toBeUndefined();
    await expect(adapter.port.documents.history.list({
      cursor: "cursor.history" as KernelPageCursor,
      limit: 5,
      locator,
      workspaceGeneration,
    })).resolves.toEqual({
      items: [{
        createdAt: "2026-07-29T23:59:00Z",
        documentLocator: locator,
        revision: "revision-1",
        sizeBytes: 8,
        snapshotId,
        workspaceGeneration,
      }],
      nextCursor: "next.history",
      workspaceGeneration,
    });
    await expect(adapter.port.documents.history.restore({
      expectedRevision,
      locator,
      snapshotId,
      workspaceGeneration,
    })).resolves.toMatchObject({
      contents: "restored",
      locator,
      workspaceGeneration,
    });

    expect(requests).toEqual([
      {
        body: {
          expectedRevision: "revision-1",
          name: "renamed.md",
          targetParent: "archive",
          workspaceGeneration: WORKSPACE_GENERATION,
        },
        method: "POST",
        pathname: "/api/v1/documents/document.signature/move",
        search: "",
      },
      {
        body: {
          deletionPolicy: "recoverable",
          expectedRevision: "revision-1",
          workspaceGeneration: WORKSPACE_GENERATION,
        },
        method: "POST",
        pathname: "/api/v1/documents/document.signature/delete",
        search: "",
      },
      {
        body: undefined,
        method: "GET",
        pathname: "/api/v1/documents/document.signature/history",
        search: "?cursor=cursor.history&limit=5",
      },
      {
        body: {
          expectedRevision: "revision-1",
          workspaceGeneration: WORKSPACE_GENERATION,
        },
        method: "POST",
        pathname: `/api/v1/documents/document.signature/history/${snapshotId}/restore`,
        search: "",
      },
    ]);
  });

  it("reads history and resource bodies through the authenticated Kernel", async () => {
    const snapshotId = "123e4567-e89b-42d3-a456-426614174050" as KernelHistorySnapshotId;
    const requests: Array<{ authorization: string | null; pathname: string; search: string }> = [];
    const fetch: FetchLike = async (url, init = {}) => {
      const parsed = new URL(url);
      if (
        parsed.pathname.includes("/history/") ||
        parsed.pathname === "/api/v1/inventory" ||
        parsed.pathname.startsWith("/api/v1/resources/")
      ) {
        requests.push({
          authorization: new Headers(init.headers).get("authorization"),
          pathname: parsed.pathname,
          search: parsed.search,
        });
      }
      if (parsed.pathname.endsWith(`/history/${snapshotId}`)) {
        return jsonResponse({
          contents: "previous note",
          createdAt: "2026-07-30T00:00:00Z",
          documentId: "document.signature",
          revision: "revision-0",
          sizeBytes: 13,
          snapshotId,
        });
      }
      if (parsed.pathname === "/api/v1/inventory") {
        return jsonResponse({
          items: [{
            entryType: "resource",
            resource: {
              id: "resource.signature",
              kind: "image",
              mediaType: "image/png",
              modifiedAt: "2026-07-30T00:00:00Z",
              name: "cover.png",
              parent: "assets",
              path: "assets/cover.png",
              previewable: true,
              revision: "revision-resource-1",
              sizeBytes: 11,
            },
          }],
          nextCursor: null,
        });
      }
      if (parsed.pathname === "/api/v1/resources/resource.signature") {
        return new Response("image bytes", {
          headers: {
            "content-length": "11",
            "content-type": "image/png",
            "x-content-type-options": "nosniff",
            "x-request-id": REQUEST_ID,
          },
        });
      }
      return handshakeResponse(parsed.pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const workspaceGeneration = WORKSPACE_GENERATION as KernelWorkspaceGeneration;

    await expect(adapter.port.documents.history.read({
      locator: "document.signature" as KernelDocumentLocator,
      snapshotId,
      workspaceGeneration,
    })).resolves.toEqual({
      contents: "previous note",
      documentLocator: "document.signature",
      revision: "revision-0",
      snapshotId,
      workspaceGeneration,
    });
    await expect(adapter.port.resources.list({
      parent: "assets" as KernelWorkspaceRelativePath,
      workspaceGeneration,
    })).resolves.toEqual({
      items: [{
        entryType: "resource",
        resource: {
          id: "resource.signature",
          kind: "image",
          mediaType: "image/png",
          modifiedAt: "2026-07-30T00:00:00Z",
          name: "cover.png",
          parent: "assets",
          previewable: true,
          relativePath: "assets/cover.png",
          revision: "revision-resource-1",
          sizeBytes: 11,
          workspaceGeneration,
        },
      }],
      workspaceGeneration,
    });
    const body = await adapter.port.resources.open({
      id: "resource.signature",
      kind: "image",
      workspaceGeneration,
    });
    expect(body.mediaType).toBe("image/png");
    expect(await body.body.text()).toBe("image bytes");
    expect(requests).toEqual([
      {
        authorization: `Bearer ${CREDENTIAL}`,
        pathname: `/api/v1/documents/document.signature/history/${snapshotId}`,
        search: "",
      },
      {
        authorization: `Bearer ${CREDENTIAL}`,
        pathname: "/api/v1/inventory",
        search: "?limit=100&parent=assets",
      },
      {
        authorization: `Bearer ${CREDENTIAL}`,
        pathname: "/api/v1/resources/resource.signature",
        search: "?kind=image",
      },
    ]);
  });

  it("writes raw resource blobs through the authenticated Kernel and maps the frozen generation", async () => {
    const requests: Array<{
      authorization: string | null;
      body: BodyInit | null | undefined;
      contentType: string | null;
      pathname: string;
      query: Record<string, string>;
    }> = [];
    const fetch: FetchLike = async (url, init = {}) => {
      const parsed = new URL(url);
      if (parsed.pathname === "/api/v1/documents/document.signature/resources") {
        requests.push({
          authorization: new Headers(init.headers).get("authorization"),
          body: init.body,
          contentType: new Headers(init.headers).get("content-type"),
          pathname: parsed.pathname,
          query: Object.fromEntries(parsed.searchParams),
        });
        return jsonResponse({
          id: "resource.signature",
          kind: "image",
          mediaType: "image/png",
          modifiedAt: "2026-07-31T00:00:00Z",
          name: "pasted.png",
          parent: "assets",
          path: "assets/pasted.png",
          previewable: true,
          revision: "sha256:resource-revision",
          sizeBytes: 8,
        }, 201);
      }
      return handshakeResponse(parsed.pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const body = new Blob([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], {
      type: "image/png",
    });
    const workspaceGeneration = WORKSPACE_GENERATION as KernelWorkspaceGeneration;

    await expect(adapter.port.resources.create({
      body,
      documentLocator: "document.signature" as KernelDocumentLocator,
      folder: "assets" as KernelWorkspaceRelativePath,
      kind: "image",
      mediaType: "image/png",
      name: "pasted.png",
      workspaceGeneration,
    })).resolves.toEqual({
      id: "resource.signature",
      kind: "image",
      mediaType: "image/png",
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "pasted.png",
      parent: "assets",
      previewable: true,
      relativePath: "assets/pasted.png",
      revision: "sha256:resource-revision",
      sizeBytes: 8,
      workspaceGeneration,
    });
    expect(requests).toEqual([{
      authorization: `Bearer ${CREDENTIAL}`,
      body,
      contentType: "image/png",
      pathname: "/api/v1/documents/document.signature/resources",
      query: {
        folder: "assets",
        kind: "image",
        name: "pasted.png",
        workspaceGeneration: WORKSPACE_GENERATION,
      },
    }]);
  });

  it("fails closed when the adapter retires while a resource body is being consumed", async () => {
    let resolveBody: ((body: Blob) => unknown) | undefined;
    let delayedResponse: Response | undefined;
    const bodyStarted = new Promise<undefined>((resolve) => {
      const response = new Response("pending", {
        headers: {
          "content-length": "7",
          "content-type": "image/png",
          "x-content-type-options": "nosniff",
          "x-request-id": REQUEST_ID,
        },
      });
      Object.defineProperty(response, "blob", {
        value: () => {
          resolve(undefined);
          return new Promise<Blob>((resolveBlob) => {
            resolveBody = resolveBlob;
          });
        },
      });
      delayedResponse = response;
    });
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname.startsWith("/api/v1/resources/")) {
        if (delayedResponse === undefined) throw new Error("resource response unavailable");
        return delayedResponse;
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const opening = adapter.port.resources.open({
      id: "resource.signature",
      kind: "image",
      workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
    });

    await bodyStarted;
    adapter.release();
    resolveBody?.(new Blob(["retired"], { type: "image/png" }));

    await expect(opening).rejects.toMatchObject({ code: "released" });
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

  it("rejects stale generations across the complete document-tree slice before transport", async () => {
    let documentRequests = 0;
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/search" || pathname.startsWith("/api/v1/documents")) {
        documentRequests += 1;
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });
    const staleGeneration = "123e4567-e89b-42d3-a456-426614174099" as KernelWorkspaceGeneration;
    const locator = "document.signature" as KernelDocumentLocator;
    const expectedRevision = "revision-1" as KernelRevision;
    const parent = "notes" as KernelWorkspaceRelativePath;
    const snapshotId = "123e4567-e89b-42d3-a456-426614174050" as KernelHistorySnapshotId;
    const operations: Array<() => Promise<unknown>> = [
      () => adapter.port.documents.list({ workspaceGeneration: staleGeneration }),
      () => adapter.port.documents.search({ query: "needle", workspaceGeneration: staleGeneration }),
      () => adapter.port.documents.create({
        contents: "created",
        kind: "file",
        name: "created.md",
        parent,
        workspaceGeneration: staleGeneration,
      }),
      () => adapter.port.documents.move({
        expectedRevision,
        locator,
        name: "renamed.md",
        targetParent: parent,
        workspaceGeneration: staleGeneration,
      }),
      () => adapter.port.documents.delete({
        deletionPolicy: "permanent",
        expectedRevision,
        locator,
        workspaceGeneration: staleGeneration,
      }),
      () => adapter.port.documents.history.list({
        locator,
        workspaceGeneration: staleGeneration,
      }),
      () => adapter.port.documents.history.restore({
        expectedRevision,
        locator,
        snapshotId,
        workspaceGeneration: staleGeneration,
      }),
    ];

    for (const operation of operations) {
      await expect(operation()).rejects.toMatchObject({
        code: "workspace-generation-mismatch",
      });
    }
    expect(documentRequests).toBe(0);
  });

  it("permanently fails closed when the workspace identity drifts after a page response", async () => {
    let workspaceReads = 0;
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/workspace") {
        workspaceReads += 1;
        return jsonResponse(workspaceReads <= 2 ? workspaceBody() : {
          ...workspaceBody(),
          id: "123e4567-e89b-42d3-a456-426614174099",
        });
      }
      if (pathname === "/api/v1/documents") {
        return jsonResponse({ items: [], nextCursor: null });
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(adapter.port.documents.list({
      workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    await expect(adapter.port.runtime.read()).rejects.toMatchObject({ code: "released" });
    expect(release).toHaveBeenCalledOnce();
  });

  it("permanently fails closed before issuing a mutation when workspace identity already drifted", async () => {
    let workspaceReads = 0;
    let documentRequests = 0;
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/workspace") {
        workspaceReads += 1;
        return jsonResponse(workspaceReads === 1 ? workspaceBody() : {
          ...workspaceBody(),
          generation: "123e4567-e89b-42d3-a456-426614174099",
        });
      }
      if (pathname === "/api/v1/documents") {
        documentRequests += 1;
        return jsonResponse({
          ...documentEntry("created.file", "file", {
            name: "created.md",
            path: "notes/created.md",
          }),
          contents: "created",
        }, 201);
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(adapter.port.documents.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: "notes" as KernelWorkspaceRelativePath,
      workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(documentRequests).toBe(0);
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
    expect(release).toHaveBeenCalledOnce();
  });

  it("permanently fails closed when history responds with a different document identity", async () => {
    const release = vi.fn(() => undefined);
    const snapshotId = "123e4567-e89b-42d3-a456-426614174050";
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname.endsWith("/history")) {
        return jsonResponse({
          items: [{
            createdAt: "2026-07-29T23:59:00Z",
            documentId: "different.signature",
            revision: "revision-1",
            sizeBytes: 8,
            snapshotId,
          }],
          nextCursor: null,
        });
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(adapter.port.documents.history.list({
      locator: "document.signature" as KernelDocumentLocator,
      workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
    expect(release).toHaveBeenCalledOnce();
  });

  it("permanently fails closed when create response identity disagrees with the requested path", async () => {
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/documents") {
        return jsonResponse({
          ...documentEntry("created.file", "file", {
            name: "created.md",
            parent: "notes",
            path: "notes/different.md",
          }),
          contents: "created",
        }, 201);
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(adapter.port.documents.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: "notes" as KernelWorkspaceRelativePath,
      workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
    expect(release).toHaveBeenCalledOnce();
  });

  it("permanently fails closed when move response identity disagrees with the requested path", async () => {
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname.endsWith("/move")) {
        return jsonResponse(documentEntry("document.signature", "file", {
          name: "renamed.md",
          parent: "archive",
          path: "archive/different.md",
        }));
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(adapter.port.documents.move({
      expectedRevision: "revision-1" as KernelRevision,
      locator: "document.signature" as KernelDocumentLocator,
      name: "renamed.md",
      targetParent: "archive" as KernelWorkspaceRelativePath,
      workspaceGeneration: WORKSPACE_GENERATION as KernelWorkspaceGeneration,
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    await expect(adapter.port.workspace.read()).rejects.toMatchObject({ code: "released" });
    expect(release).toHaveBeenCalledOnce();
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

  it("routes settings and sync through the authenticated Kernel with explicit revisions", async () => {
    const requests: Array<{ body: unknown; method: string; pathname: string }> = [];
    const fetch: FetchLike = async (url, init = {}) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/settings") {
        requests.push({
          body: init.body === undefined ? undefined : JSON.parse(String(init.body)),
          method: String(init.method),
          pathname,
        });
        return jsonResponse(settingsBody(init.method === "PATCH" ? "settings-2" : "settings-1"));
      }
      if (pathname === "/api/v1/sync/config") {
        requests.push({
          body: init.body === undefined ? undefined : JSON.parse(String(init.body)),
          method: String(init.method),
          pathname,
        });
        return jsonResponse(syncConfigBody(init.method === "PATCH" ? "sync-2" : "sync-1"));
      }
      if (pathname === "/api/v1/sync/connection-test") {
        requests.push({ body: JSON.parse(String(init.body)), method: String(init.method), pathname });
        return jsonResponse({
          checkedTarget: "s3://redacted",
          configRevision: "sync-2",
          provider: "s3",
        });
      }
      if (pathname === "/api/v1/sync/status") {
        requests.push({ body: undefined, method: String(init.method), pathname });
        return jsonResponse({
          activeRunId: null,
          completionState: "idle",
          configRevision: "sync-2",
          error: null,
          lastAttemptAt: null,
          lastSuccessfulSyncAt: null,
          lastTrigger: null,
          provider: "s3",
          summary: null,
        });
      }
      if (pathname === "/api/v1/sync/runs") {
        requests.push({ body: JSON.parse(String(init.body)), method: String(init.method), pathname });
        return jsonResponse({
          acceptedAt: "2026-07-30T00:00:00Z",
          configRevision: "sync-2",
          runId: "123e4567-e89b-42d3-a456-426614174060",
        }, 202);
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection(), { fetch });

    await expect(adapter.port.settings.read()).resolves.toEqual(settingsBody("settings-1"));
    await expect(adapter.port.settings.patch({
      expectedRevision: "settings-1" as KernelRevision,
      token: "must-not-cross-the-domain-boundary",
      values: [{ key: "language", value: { type: "string", value: "zh-CN" } }],
    } as Parameters<typeof adapter.port.settings.patch>[0])).resolves.toEqual(
      settingsBody("settings-2"),
    );
    await expect(adapter.port.sync.readConfig()).resolves.toEqual(syncConfigBody("sync-1"));
    await expect(adapter.port.sync.patchConfig({
      baseUrl: "http://must-not-cross.invalid",
      changes: { enabled: true, token: "must-not-cross" },
      expectedRevision: "sync-1" as KernelRevision,
    } as Parameters<typeof adapter.port.sync.patchConfig>[0])).resolves.toMatchObject({
      revision: "sync-2",
    });
    await expect(adapter.port.sync.testConnection({
      changes: {
        endpoint: "must-not-cross",
        s3AccessKeyId: { operation: "keep", token: "must-not-cross" },
      },
      expectedRevision: "sync-2" as KernelRevision,
    } as Parameters<typeof adapter.port.sync.testConnection>[0])).resolves.toEqual({
      checkedTarget: "s3://redacted",
      configRevision: "sync-2",
      provider: "s3",
    });
    await expect(adapter.port.sync.readStatus()).resolves.toMatchObject({
      completionState: "idle",
      configRevision: "sync-2",
    });
    await expect(
      adapter.port.sync.trigger("sync-2" as KernelRevision),
    ).resolves.toMatchObject({ configRevision: "sync-2" });

    expect(requests).toEqual([
      { body: undefined, method: "GET", pathname: "/api/v1/settings" },
      {
        body: {
          expectedRevision: "settings-1",
          values: [{ key: "language", value: { type: "string", value: "zh-CN" } }],
        },
        method: "PATCH",
        pathname: "/api/v1/settings",
      },
      { body: undefined, method: "GET", pathname: "/api/v1/sync/config" },
      {
        body: { changes: { enabled: true }, expectedRevision: "sync-1" },
        method: "PATCH",
        pathname: "/api/v1/sync/config",
      },
      {
        body: {
          changes: { s3AccessKeyId: { operation: "keep" } },
          expectedRevision: "sync-2",
        },
        method: "POST",
        pathname: "/api/v1/sync/connection-test",
      },
      { body: undefined, method: "GET", pathname: "/api/v1/sync/status" },
      {
        body: { expectedConfigRevision: "sync-2" },
        method: "POST",
        pathname: "/api/v1/sync/runs",
      },
    ]);
    expect(JSON.stringify(requests)).not.toContain(CREDENTIAL);
    expect(JSON.stringify(requests)).not.toContain(BASE_URL);
  });

  it.each([
    ["settings read", (port: KernelDomainPort) => port.settings.read()],
    [
      "settings patch",
      (port: KernelDomainPort) =>
        port.settings.patch({
          expectedRevision: "settings-1" as KernelRevision,
          values: [],
        }),
    ],
    ["sync config read", (port: KernelDomainPort) => port.sync.readConfig()],
    [
      "sync config patch",
      (port: KernelDomainPort) =>
        port.sync.patchConfig({ changes: {}, expectedRevision: "sync-1" as KernelRevision }),
    ],
    ["sync status read", (port: KernelDomainPort) => port.sync.readStatus()],
    [
      "sync connection test",
      (port: KernelDomainPort) =>
        port.sync.testConnection({ changes: {}, expectedRevision: "sync-1" as KernelRevision }),
    ],
    [
      "sync trigger",
      (port: KernelDomainPort) => port.sync.trigger("sync-1" as KernelRevision),
    ],
  ])("blocks %s before transport when workspace identity drifted", async (_name, operation) => {
    let workspaceReads = 0;
    let domainRequests = 0;
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/workspace") {
        workspaceReads += 1;
        return jsonResponse(workspaceReads === 1 ? workspaceBody() : {
          ...workspaceBody(),
          generation: "123e4567-e89b-42d3-a456-426614174099",
        });
      }
      if (pathname === "/api/v1/settings" || pathname.startsWith("/api/v1/sync/")) {
        domainRequests += 1;
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(operation(adapter.port)).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(domainRequests).toBe(0);
    expect(release).toHaveBeenCalledOnce();
  });

  it("closes after a settings mutation if the postflight workspace identity drifts", async () => {
    let workspaceReads = 0;
    let settingsRequests = 0;
    const release = vi.fn(() => undefined);
    const fetch: FetchLike = async (url) => {
      const pathname = new URL(url).pathname;
      if (pathname === "/api/v1/workspace") {
        workspaceReads += 1;
        return jsonResponse(workspaceReads <= 2 ? workspaceBody() : {
          ...workspaceBody(),
          id: "123e4567-e89b-42d3-a456-426614174099",
        });
      }
      if (pathname === "/api/v1/settings") {
        settingsRequests += 1;
        return jsonResponse(settingsBody("settings-2"));
      }
      return handshakeResponse(pathname);
    };
    const adapter = await createDesktopKernelDomainAdapter(connection({ release }), { fetch });

    await expect(adapter.port.settings.patch({
      expectedRevision: "settings-1" as KernelRevision,
      values: [],
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(settingsRequests).toBe(1);
    await expect(adapter.port.sync.readStatus()).rejects.toMatchObject({ code: "released" });
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
  if (pathname === "/api/v1/app-config") {
    return jsonResponse(appConfigBody());
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
      resources: true,
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

function settingsBody(revision: string) {
  return {
    revision,
    values: [{ key: "language" as const, value: { type: "string" as const, value: "zh-CN" } }],
  };
}

function appConfigBody({
  generation = WORKSPACE_GENERATION,
  workspaceId = WORKSPACE_ID,
}: {
  generation?: string;
  workspaceId?: string;
} = {}) {
  return {
    appConfigVersion: 1 as const,
    localState: {
      fileTreeSort: { direction: "ascending" as const, key: "name" as const },
      pandocPath: null,
      recentMarkdownFiles: [{ name: "Draft", path: "notes/draft.md" }],
      revision: "app-config-revision-1",
      uiLayout: {
        openWindows: [],
        schemaVersion: 1 as const,
        windowStates: {},
      },
    },
    settings: settingsBody("settings-1"),
    workspace: { generation, id: workspaceId },
  };
}

function syncConfigBody(revision: string) {
  return {
    configured: true,
    enabled: true,
    generateConflictDocument: true,
    intervalSeconds: 300,
    issues: [],
    mode: "automatic" as const,
    provider: "s3" as const,
    readiness: "ready" as const,
    remoteRoot: "qingyu",
    revision,
    s3: {
      accessKeyId: { present: true },
      addressingStyle: "path" as const,
      bucket: "notes",
      endpointUrl: { redacted: true, value: null },
      region: "test-1",
      requestTimeoutSeconds: 60,
      secretAccessKey: { present: true },
      tlsVerification: "verify" as const,
    },
    webdav: {
      password: { present: false },
      serverUrl: { redacted: false, value: null },
      username: "",
    },
  };
}

function documentEntry(
  id: string,
  kind: "file" | "directory",
  overrides: Record<string, unknown> = {},
) {
  return {
    id,
    kind,
    modifiedAt: "2026-07-30T00:00:00Z",
    name: kind === "file" ? "draft.md" : "folder",
    parent: "notes",
    path: kind === "file" ? "notes/draft.md" : "notes/folder",
    revision: "revision-1",
    sizeBytes: kind === "file" ? 8 : 0,
    ...overrides,
  };
}

function documentSnapshot(
  locator: string,
  kind: "file" | "directory",
  workspaceGeneration: KernelWorkspaceGeneration,
  overrides: Record<string, unknown> = {},
) {
  return {
    kind,
    locator,
    modifiedAt: "2026-07-30T00:00:00Z",
    name: kind === "file" ? "draft.md" : "folder",
    parent: "notes",
    relativePath: kind === "file" ? "notes/draft.md" : "notes/folder",
    revision: "revision-1",
    sizeBytes: kind === "file" ? 8 : 0,
    workspaceGeneration,
    ...overrides,
  };
}

function jsonResponse(body: unknown, status = 200) {
  return Response.json(body, {
    status,
    headers: { "x-request-id": REQUEST_ID },
  });
}
