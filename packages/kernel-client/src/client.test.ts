import { describe, expect, it, vi } from "vitest";

import {
  createKernelClient,
  KernelProtocolError,
  type FetchLike,
  type KernelClient,
} from "./index.ts";

describe("createKernelClient", () => {
  it("maps the six API groups to all frozen HTTP operations", async () => {
    const calls: Array<{ url: URL; init: RequestInit }> = [];
    const fetch: FetchLike = async (url, init = {}) => {
      calls.push({ url: new URL(url), init });
      return operationResponse(new URL(url).pathname, String(init.method));
    };
    const client = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch,
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });
    const signal = new AbortController().signal;

    await client.system.live({ signal });
    await client.system.ready({ signal });
    await client.system.version({ signal });
    await client.system.runtime({ signal });
    await client.workspace.get({ signal });
    await client.workspace.search(
      { query: "needle", cursor: "search-cursor", limit: 20 },
      { signal },
    );
    await client.resources.list(
      { parent: "notes", cursor: "resource-cursor", limit: 10 },
      { signal },
    );
    const resourceResponse = await client.resources.open("resource/1", "image", { signal });
    expect(await resourceResponse.text()).toBe("image bytes");
    await client.documents.list(
      { parent: "notes", cursor: "document-cursor", limit: 10 },
      { signal },
    );
    await client.documents.create(
      {
        kind: "file",
        workspaceGeneration: "generation-1",
        parent: "notes",
        name: "entry.md",
        contents: "created",
      },
      { signal },
    );
    await client.documents.get("document/1", { signal });
    await client.documents.update(
      "document/1",
      {
        workspaceGeneration: "generation-1",
        expectedRevision: "revision-1",
        contents: "updated",
      },
      { signal },
    );
    await client.documents.move(
      "document/1",
      {
        workspaceGeneration: "generation-1",
        expectedRevision: "revision-2",
        targetParent: "archive",
        name: "renamed.md",
      },
      { signal },
    );
    await client.documents.delete(
      "document/1",
      {
        workspaceGeneration: "generation-1",
        expectedRevision: "revision-3",
        deletionPolicy: "recoverable",
      },
      { signal },
    );
    await client.documents.listHistory(
      "document/1",
      { cursor: "history-cursor", limit: 5 },
      { signal },
    );
    await client.documents.getHistory(
      "document/1",
      "snapshot/1",
      { signal },
    );
    await client.documents.restoreHistory(
      "document/1",
      "snapshot/1",
      {
        workspaceGeneration: "generation-1",
        expectedRevision: "revision-4",
      },
      { signal },
    );
    await client.settings.get({ signal });
    await client.settings.patch(
      {
        expectedRevision: "settings-1",
        values: [{ key: "language", value: { type: "string", value: "zh-CN" } }],
      },
      { signal },
    );
    await client.sync.getConfig({ signal });
    await client.sync.patchConfig(
      { expectedRevision: "sync-1", changes: { enabled: true } },
      { signal },
    );
    await client.sync.testConnection(
      { expectedRevision: "sync-2", changes: { remoteRoot: "notes" } },
      { signal },
    );
    await client.sync.getStatus({ signal });
    await client.sync.trigger(
      { expectedConfigRevision: "sync-3" },
      { signal },
    );

    expect(calls.map(({ url, init }) => `${init.method} ${url.pathname}${url.search}`)).toEqual([
      "GET /api/v1/health/live",
      "GET /api/v1/health/ready",
      "GET /api/v1/system/version",
      "GET /api/v1/runtime",
      "GET /api/v1/workspace",
      "GET /api/v1/search?query=needle&cursor=search-cursor&limit=20",
      "GET /api/v1/inventory?parent=notes&cursor=resource-cursor&limit=10",
      "GET /api/v1/resources/resource%2F1?kind=image",
      "GET /api/v1/documents?parent=notes&cursor=document-cursor&limit=10",
      "POST /api/v1/documents",
      "GET /api/v1/documents/document%2F1",
      "PUT /api/v1/documents/document%2F1",
      "POST /api/v1/documents/document%2F1/move",
      "POST /api/v1/documents/document%2F1/delete",
      "GET /api/v1/documents/document%2F1/history?cursor=history-cursor&limit=5",
      "GET /api/v1/documents/document%2F1/history/snapshot%2F1",
      "POST /api/v1/documents/document%2F1/history/snapshot%2F1/restore",
      "GET /api/v1/settings",
      "PATCH /api/v1/settings",
      "GET /api/v1/sync/config",
      "PATCH /api/v1/sync/config",
      "POST /api/v1/sync/connection-test",
      "GET /api/v1/sync/status",
      "POST /api/v1/sync/runs",
    ]);
    expect(new Headers(calls[0]?.init.headers).has("authorization")).toBe(false);
    for (const call of calls.slice(1)) {
      expect(new Headers(call.init.headers).get("authorization")).toBe(
        "Bearer credential-1",
      );
      expect(call.init.signal).toBe(signal);
    }
    expect(JSON.parse(String(calls[11]?.init.body))).toMatchObject({
      expectedRevision: "revision-1",
    });
    expect(JSON.parse(String(calls[18]?.init.body))).toMatchObject({
      expectedRevision: "settings-1",
    });
    expect(JSON.parse(String(calls[23]?.init.body))).toMatchObject({
      expectedConfigRevision: "sync-3",
    });
  });

  it("keeps base URL, credential provider, and fetch injection instance-local", async () => {
    const firstFetch = vi.fn<FetchLike>(async () =>
      jsonResponse({ apiVersion: "v1", instanceId: "123e4567-e89b-42d3-a456-426614174001", kernelVersion: "1" }),
    );
    const secondFetch = vi.fn<FetchLike>(async () =>
      jsonResponse({ apiVersion: "v1", instanceId: "123e4567-e89b-42d3-a456-426614174002", kernelVersion: "1" }),
    );
    const first = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: firstFetch,
      auth: { kind: "native-bearer", getCredential: () => "first-token" },
    });
    const second = createKernelClient({
      baseUrl: "http://127.0.0.1:7708",
      fetch: secondFetch,
      auth: { kind: "native-bearer", getCredential: () => "second-token" },
    });

    await Promise.all([first.system.version(), second.system.version()]);

    expect(firstFetch.mock.calls[0]?.[0]).toBe(
      "http://127.0.0.1:6608/api/v1/system/version",
    );
    expect(secondFetch.mock.calls[0]?.[0]).toBe(
      "http://127.0.0.1:7708/api/v1/system/version",
    );
    expect(new Headers(firstFetch.mock.calls[0]?.[1]?.headers).get("authorization")).toBe(
      "Bearer first-token",
    );
    expect(new Headers(secondFetch.mock.calls[0]?.[1]?.headers).get("authorization")).toBe(
      "Bearer second-token",
    );
  });

  it("accepts canonical nil UUIDs produced by the Rust UUID wire type", async () => {
    const client = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () =>
        jsonResponse({
          apiVersion: "v1",
          instanceId: "00000000-0000-0000-0000-000000000000",
          kernelVersion: "1",
        }),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });

    await expect(client.system.version()).resolves.toMatchObject({
      instanceId: "00000000-0000-0000-0000-000000000000",
    });
  });

  it("does not expose a cookie or session authentication branch", () => {
    if (false) {
      createKernelClient({
        baseUrl: "http://127.0.0.1:6608",
        fetch: async () => Response.json({}),
        // @ts-expect-error Phase 1 supports only the native launch bearer.
        auth: { kind: "cookie-session" },
      });
    }
    expect(true).toBe(true);
  });

  it("rejects malformed success bodies for every frozen HTTP operation", async () => {
    const calls = operationCalls(
      createKernelClient({
        baseUrl: "http://127.0.0.1:6608",
        fetch: async () => jsonResponse({ unexpected: "unsafe" }),
        auth: { kind: "native-bearer", getCredential: () => "credential-1" },
      }),
    );

    for (const call of calls) {
      await expect(call()).rejects.toBeInstanceOf(KernelProtocolError);
    }
  });

  it("enforces each operation's exact success status including delete-only 204", async () => {
    const liveWithWrongStatus = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () =>
        jsonResponse({ apiVersion: "v1", status: "live" }, { status: 201 }),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });
    await expect(liveWithWrongStatus.system.live()).rejects.toBeInstanceOf(
      KernelProtocolError,
    );

    const settingsWith204 = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () => emptyResponse(),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });
    await expect(settingsWith204.settings.get()).rejects.toBeInstanceOf(
      KernelProtocolError,
    );

    const deleteWith200 = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () => jsonResponse({}, { status: 200 }),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });
    await expect(
      deleteWith200.documents.delete("payload.signature", {
        workspaceGeneration: "generation-1",
        expectedRevision: "revision-1",
        deletionPolicy: "recoverable",
      }),
    ).rejects.toBeInstanceOf(KernelProtocolError);
  });

  it("rejects extra, missing, out-of-range, and unsafe DTO fields", async () => {
    const invalidBodies = [
      { items: [], nextCursor: null, extra: true },
      { items: [] },
      { items: [{ ...ENTRY, sizeBytes: Number.MAX_SAFE_INTEGER + 1 }], nextCursor: null },
      { items: [{ ...ENTRY, path: "../outside.md" }], nextCursor: null },
    ];
    for (const body of invalidBodies) {
      const client = createKernelClient({
        baseUrl: "http://127.0.0.1:6608",
        fetch: async () => jsonResponse(body),
        auth: { kind: "native-bearer", getCredential: () => "credential-1" },
      });
      await expect(client.documents.list()).rejects.toBeInstanceOf(KernelProtocolError);
    }
  });

  it("rejects inconsistent or unsafe resource inventory entries", async () => {
    const resource = {
      id: "payload.signature",
      kind: "image",
      mediaType: "image/png",
      modifiedAt: "2026-07-29T12:30:45Z",
      name: "photo.png",
      parent: "assets",
      path: "assets/photo.png",
      previewable: true,
      revision: "sha256:revision",
      sizeBytes: 1024,
    };
    const invalidResources = [
      { ...resource, mediaType: "text/html" },
      { ...resource, previewable: false },
      { ...resource, name: "../photo.png" },
      { ...resource, path: "other/photo.png" },
      { ...resource, name: "photo.jpg", path: "assets/photo.jpg" },
      { ...resource, name: "photo\u0085.png", path: "assets/photo\u0085.png" },
      { ...resource, parent: "assets\u0085", path: "assets\u0085/photo.png" },
      {
        ...resource,
        kind: "attachment",
        mediaType: "image/png",
        previewable: true,
      },
    ];

    for (const candidate of invalidResources) {
      const client = createKernelClient({
        baseUrl: "http://127.0.0.1:6608",
        fetch: async () => jsonResponse({
          items: [{ entryType: "resource", resource: candidate }],
          nextCursor: null,
        }),
        auth: { kind: "native-bearer", getCredential: () => "credential-1" },
      });
      await expect(client.resources.list()).rejects.toBeInstanceOf(KernelProtocolError);
    }
  });

  it("binds binary response media types to the requested resource kind", async () => {
    const responses = [
      binaryResponse("application/octet-stream"),
      binaryResponse("image/png"),
    ];
    const client = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () => responses.shift() ?? binaryResponse("image/png"),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });

    await expect(client.resources.open("payload.signature", "image")).rejects.toBeInstanceOf(
      KernelProtocolError,
    );
    await expect(
      client.resources.open("payload.signature", "attachment"),
    ).rejects.toBeInstanceOf(KernelProtocolError);
  });

  it("rejects impossible calendar dates and 24:00 timestamps over HTTP", async () => {
    for (const modifiedAt of [
      "2026-02-31T00:00:00Z",
      "2026-04-31T12:30:45.000Z",
      "2026-01-01T24:00:00Z",
    ]) {
      const client = createKernelClient({
        baseUrl: "http://127.0.0.1:6608",
        fetch: async () =>
          Response.json(
            { items: [{ ...ENTRY, modifiedAt }], nextCursor: null },
            { headers: { "x-request-id": UUID } },
          ),
        auth: { kind: "native-bearer", getCredential: () => "credential-1" },
      });

      await expect(client.documents.list()).rejects.toBeInstanceOf(KernelProtocolError);
    }
  });

  it("accepts leap-day timestamps with a zero RFC3339 offset over HTTP", async () => {
    const client = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () =>
        Response.json(
          {
            items: [
              { ...ENTRY, modifiedAt: "2024-02-29T23:59:59.123456789+00:00" },
              { ...ENTRY, modifiedAt: "2024-02-29T23:59:59.1234567890-00:00" },
            ],
            nextCursor: null,
          },
          { headers: { "x-request-id": UUID } },
        ),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });

    await expect(client.documents.list()).resolves.toMatchObject({
      items: [
        { modifiedAt: "2024-02-29T23:59:59.123456789+00:00" },
        { modifiedAt: "2024-02-29T23:59:59.1234567890-00:00" },
      ],
    });
  });

  it("rejects unsafe sync issue text and empty sync status revisions", async () => {
    const unsafeIssueClient = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () =>
        jsonResponse({
          ...SYNC_CONFIG,
          issues: [
            {
              code: "required",
              field: "s3.secretAccessKey",
              message: "secret value copied from backend",
            },
          ],
        }),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });
    await expect(unsafeIssueClient.sync.getConfig()).rejects.toBeInstanceOf(
      KernelProtocolError,
    );

    const emptyRevisionClient = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () =>
        jsonResponse({
          activeRunId: null,
          completionState: "idle",
          configRevision: "",
          error: null,
          lastAttemptAt: null,
          lastSuccessfulSyncAt: null,
          lastTrigger: null,
          provider: "s3",
          summary: null,
        }),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });
    await expect(emptyRevisionClient.sync.getStatus()).rejects.toBeInstanceOf(
      KernelProtocolError,
    );

    const inconsistentStatusClient = createKernelClient({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () =>
        jsonResponse({
          activeRunId: null,
          completionState: "failed",
          configRevision: "sync-1",
          error: null,
          lastAttemptAt: "2026-07-29T00:00:00Z",
          lastSuccessfulSyncAt: null,
          lastTrigger: "manual",
          provider: "s3",
          summary: null,
        }),
      auth: { kind: "native-bearer", getCredential: () => "credential-1" },
    });
    await expect(inconsistentStatusClient.sync.getStatus()).rejects.toBeInstanceOf(
      KernelProtocolError,
    );
  });
});

function operationCalls(client: KernelClient): Array<() => Promise<unknown>> {
  const documentId = "payload.signature";
  const request = {
    workspaceGeneration: "generation-1",
    expectedRevision: "revision-1",
  };
  return [
    () => client.system.live(),
    () => client.system.ready(),
    () => client.system.version(),
    () => client.system.runtime(),
    () => client.workspace.get(),
    () => client.workspace.search({ query: "needle" }),
    () => client.resources.list(),
    () => client.resources.open("payload.signature", "attachment"),
    () => client.documents.list(),
    () =>
      client.documents.create({
        kind: "file",
        workspaceGeneration: request.workspaceGeneration,
        parent: "",
        name: "note.md",
        contents: "note",
      }),
    () => client.documents.get(documentId),
    () => client.documents.update(documentId, { ...request, contents: "note" }),
    () =>
      client.documents.move(documentId, {
        ...request,
        targetParent: "",
        name: "note.md",
      }),
    () =>
      client.documents.delete(documentId, {
        ...request,
        deletionPolicy: "recoverable",
      }),
    () => client.documents.listHistory(documentId),
    () => client.documents.getHistory(documentId, "snapshot-1"),
    () => client.documents.restoreHistory(documentId, "snapshot-1", request),
    () => client.settings.get(),
    () =>
      client.settings.patch({
        expectedRevision: "settings-1",
        values: [{ key: "language", value: { type: "string", value: "fr" } }],
      }),
    () => client.sync.getConfig(),
    () => client.sync.patchConfig({ expectedRevision: "sync-1", changes: { enabled: true } }),
    () =>
      client.sync.testConnection({ expectedRevision: "sync-1", changes: { enabled: true } }),
    () => client.sync.getStatus(),
    () => client.sync.trigger({ expectedConfigRevision: "sync-1" }),
  ];
}

const UUID = "123e4567-e89b-42d3-a456-426614174000";
const ENTRY = {
  id: "payload.signature",
  kind: "file",
  modifiedAt: "2026-01-01T00:00:00Z",
  name: "note.md",
  parent: "",
  path: "note.md",
  revision: "revision-1",
  sizeBytes: 4,
};
const CONTENT = { ...ENTRY, contents: "note" };
const HISTORY_SNAPSHOT = {
  contents: "previous note",
  createdAt: "2026-01-01T00:00:00Z",
  documentId: "payload.signature",
  revision: "revision-0",
  sizeBytes: 13,
  snapshotId: UUID,
};
const SETTINGS = { revision: "settings-1", values: [] };
const SYNC_CONFIG = {
  configured: true,
  enabled: true,
  generateConflictDocument: true,
  intervalSeconds: 60,
  issues: [],
  mode: "automatic",
  provider: "s3",
  readiness: "ready",
  remoteRoot: "notes",
  revision: "sync-1",
  s3: {
    accessKeyId: { present: true },
    addressingStyle: "auto",
    bucket: "bucket",
    endpointUrl: { redacted: false, value: "https://s3.example.com" },
    region: "us-east-1",
    requestTimeoutSeconds: 30,
    secretAccessKey: { present: true },
    tlsVerification: "verify",
  },
  webdav: {
    password: { present: false },
    serverUrl: { redacted: false, value: null },
    username: "",
  },
};

function operationResponse(path: string, method: string) {
  const response = operationResponseWithoutRequestId(path, method);
  response.headers.set("x-request-id", UUID);
  return response;
}

function operationResponseWithoutRequestId(path: string, method: string) {
  if (path === "/api/v1/health/live") return Response.json({ apiVersion: "v1", status: "live" });
  if (path === "/api/v1/health/ready") return Response.json({ apiVersion: "v1", instanceId: UUID, status: "ready" });
  if (path === "/api/v1/system/version") return Response.json({ apiVersion: "v1", instanceId: UUID, kernelVersion: "1" });
  if (path === "/api/v1/runtime") return Response.json({ capabilities: { documents: true, history: true, portableSettings: true, resources: true, s3: true, search: true, settings: true, sync: true, webdav: true }, instanceId: UUID, profile: "desktop", startupState: "ready" });
  if (path === "/api/v1/workspace") return Response.json({ id: UUID, generation: "generation-1", displayName: "Notes", readiness: "ready", revision: "revision-1" });
  if (path === "/api/v1/search") return Response.json({ items: [], nextCursor: null });
  if (path === "/api/v1/inventory") return Response.json({ items: [], nextCursor: null });
  if (path.includes("/resources/")) return new Response("image bytes", {
    headers: {
      "content-length": "11",
      "content-type": "image/png",
      "x-content-type-options": "nosniff",
    },
  });
  if (path === "/api/v1/documents" && method === "GET") return Response.json({ items: [], nextCursor: null });
  if (path === "/api/v1/documents" && method === "POST") return Response.json(CONTENT, { status: 201 });
  if (path.endsWith("/delete")) return new Response(null, { status: 204 });
  if (path.endsWith("/history")) return Response.json({ items: [], nextCursor: null });
  if (path.includes("/history/") && !path.endsWith("/restore")) {
    return Response.json(HISTORY_SNAPSHOT);
  }
  if (path.includes("/documents/")) return Response.json(path.endsWith("/move") ? ENTRY : CONTENT);
  if (path === "/api/v1/settings") return Response.json(SETTINGS);
  if (path === "/api/v1/sync/config") return Response.json(SYNC_CONFIG);
  if (path === "/api/v1/sync/connection-test") return Response.json({ checkedTarget: "s3", configRevision: "sync-1", provider: "s3" });
  if (path === "/api/v1/sync/status") return Response.json({ activeRunId: null, completionState: "idle", configRevision: "sync-1", error: null, lastAttemptAt: null, lastSuccessfulSyncAt: null, lastTrigger: null, provider: "s3", summary: null });
  return Response.json({ acceptedAt: "2026-01-01T00:00:00Z", configRevision: "sync-1", runId: UUID }, { status: 202 });
}

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  const headers = new Headers(init.headers);
  headers.set("x-request-id", UUID);
  return Response.json(body, { ...init, headers });
}

function binaryResponse(contentType: string) {
  return new Response("resource", {
    headers: {
      "content-length": "8",
      "content-type": contentType,
      "x-content-type-options": "nosniff",
      "x-request-id": UUID,
    },
  });
}

function emptyResponse() {
  return new Response(null, {
    status: 204,
    headers: { "x-request-id": UUID },
  });
}
