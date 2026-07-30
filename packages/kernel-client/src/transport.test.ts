import { describe, expect, it, vi } from "vitest";

import {
  KernelApiError,
  KernelProtocolError,
  KernelTransportError,
} from "./errors.ts";
import {
  KernelHttpTransport,
  type FetchLike,
  type HttpRequest,
} from "./transport.ts";

const SECRET = "native-secret-that-must-not-leak";
const REQUEST_ID = "cb80b818-31d2-4770-9cba-241fec471bd4";

describe("KernelHttpTransport", () => {
  it("puts the current native credential only in the bearer header and forwards AbortSignal", async () => {
    const controller = new AbortController();
    const fetch = vi.fn<FetchLike>(async (url, init) => {
      expect(url).toBe("http://127.0.0.1:6608/api/v1/search?query=a%2Fb+%26+c&limit=20");
      expect(url).not.toContain(SECRET);
      expect(new Headers(init?.headers).get("authorization")).toBe(`Bearer ${SECRET}`);
      expect(init?.signal).toBe(controller.signal);
      expect(init?.redirect).toBe("error");
      return jsonResponse({ items: [], nextCursor: null });
    });
    const transport = createTransport(fetch);

    await transport.request({
      method: "GET",
      path: "/api/v1/search",
      query: { query: "a/b & c", limit: 20, cursor: undefined },
      signal: controller.signal,
    });

    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("supports unauthenticated live health without reading the credential", async () => {
    const getCredential = vi.fn(() => SECRET);
    const fetch = vi.fn<FetchLike>(async (_url, init) => {
      expect(new Headers(init?.headers).has("authorization")).toBe(false);
      return jsonResponse({ apiVersion: "v1", status: "live" });
    });
    const transport = new KernelHttpTransport({
      baseUrl: "http://127.0.0.1:6608",
      fetch,
      auth: { kind: "native-bearer", getCredential },
    });

    await transport.request({
      method: "GET",
      path: "/api/v1/health/live",
      authenticated: false,
    });

    expect(getCredential).not.toHaveBeenCalled();
  });

  it("returns JSON values and treats 204 as an empty successful result", async () => {
    const responses = [
      jsonResponse({ revision: "revision-2" }),
      emptyResponse(),
    ];
    const transport = createTransport(async () => responses.shift()!);

    await expect(
      transport.request<{ revision: string }>({ method: "GET", path: "/api/v1/settings" }),
    ).resolves.toEqual({ revision: "revision-2" });
    await expect(
      transport.request({
        method: "POST",
        path: "/api/v1/documents/document-1/delete",
        body: {
          workspaceGeneration: "generation-1",
          expectedRevision: "revision-1",
          deletionPolicy: "recoverable",
        },
      }),
    ).resolves.toBeUndefined();
  });

  it("returns a validated binary response without consuming its body", async () => {
    const response = new Response("image bytes", {
      headers: {
        "content-length": "11",
        "content-type": "image/png",
        "x-content-type-options": "nosniff",
        "x-request-id": REQUEST_ID,
      },
    });
    const transport = createTransport(async () => response);

    const received = await transport.requestBinary(
      { method: "GET", path: "/api/v1/resources/signed-resource" },
      { status: 200, mediaTypes: ["image/png"] },
    );

    expect(received).toBe(response);
    expect(received.bodyUsed).toBe(false);
    await expect(received.text()).resolves.toBe("image bytes");
  });

  it("rejects malformed binary response metadata before exposing the body", async () => {
    const invalidResponses = [
      binaryResponse({ "content-type": "text/html" }),
      binaryResponse({ "content-length": "11.0" }),
      binaryResponse({ "x-content-type-options": "sniff" }),
      binaryResponse({}, 201),
    ];

    for (const response of invalidResponses) {
      const transport = createTransport(async () => response);
      await expect(
        transport.requestBinary(
          { method: "GET", path: "/api/v1/resources/signed-resource" },
          { status: 200, mediaTypes: ["image/png"] },
        ),
      ).rejects.toBeInstanceOf(KernelProtocolError);
      expect(response.bodyUsed).toBe(false);
    }
  });

  it("maps a missing binary resource to the frozen typed API error", async () => {
    const transport = createTransport(async () =>
      Response.json(
        {
          code: "resource_not_found",
          message: "The resource was not found.",
          requestId: REQUEST_ID,
        },
        { status: 404, headers: { "x-request-id": REQUEST_ID } },
      ),
    );

    await expect(
      transport.requestBinary(
        { method: "GET", path: "/api/v1/resources/signed-resource" },
        { status: 200, mediaTypes: ["image/png"] },
      ),
    ).rejects.toMatchObject({
      code: "resource_not_found",
      status: 404,
      requestId: REQUEST_ID,
    });
  });

  it("exposes only the structured API error and matching server request ID", async () => {
    const requestId = "31af5e83-d3c3-4cc4-bac7-ac865252fb19";
    const transport = createTransport(async () =>
      Response.json(
        {
          code: "revision_conflict",
          message: "The document changed since it was loaded.",
          requestId,
          details: { type: "revision-conflict", currentRevision: "revision-2" },
        },
        { status: 409, headers: { "x-request-id": requestId } },
      ),
    );

    const error = await transport
      .request({ method: "PATCH", path: "/api/v1/settings", body: { private: SECRET } })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(KernelApiError);
    expect(error).toMatchObject({
      code: "revision_conflict",
      status: 409,
      requestId,
      details: { type: "revision-conflict", currentRevision: "revision-2" },
    });
    expect(error).toMatchObject({ message: "The document changed since it was loaded." });
    expect(String(error)).not.toContain(SECRET);
    expect(JSON.stringify(error)).not.toContain(SECRET);
  });

  it("requires a canonical request ID header on successful responses", async () => {
    for (const headers of [undefined, { "x-request-id": "not-a-uuid" }]) {
      const transport = createTransport(async () =>
        Response.json({ apiVersion: "v1", status: "live" }, { headers }),
      );

      const error = await transport
        .request({ method: "GET", path: "/api/v1/health/live", authenticated: false })
        .catch((caught: unknown) => caught);

      expect(error).toBeInstanceOf(KernelProtocolError);
      expect(error).toMatchObject({ kind: "invalid-http-response", status: 200 });
    }
  });

  it("rejects error envelopes when the request ID header is missing or mismatched", async () => {
    for (const headers of [
      undefined,
      { "x-request-id": "d92c6c89-bb5b-4622-bd4f-4aeb519576b8" },
    ]) {
      const transport = createTransport(async () =>
        Response.json(
          {
            code: "unauthorized",
            message: "Authentication is required.",
            requestId: REQUEST_ID,
          },
          { status: 401, headers },
        ),
      );

      await expect(
        transport.request({ method: "GET", path: "/api/v1/workspace" }),
      ).rejects.toBeInstanceOf(KernelProtocolError);
    }
  });

  it("rejects malformed responses as protocol errors without retaining the raw body", async () => {
    const rawBody = `upstream leaked ${SECRET}`;
    const requestId = "4d30cf34-9b94-4645-9e29-d60ea25fca77";
    const transport = createTransport(
      async () =>
        new Response(rawBody, {
          status: 502,
          headers: { "x-request-id": requestId },
        }),
    );

    const error = await transport
      .request({ method: "GET", path: "/api/v1/workspace" })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(KernelProtocolError);
    expect(error).toMatchObject({
      kind: "invalid-http-response",
      requestId,
      status: 502,
    });
    expect(String(error)).not.toContain(rawBody);
    expect(JSON.stringify(error)).not.toContain(rawBody);
  });

  it("rejects unknown API error codes instead of exposing them as typed errors", async () => {
    const requestId = "8a2560cf-88fc-4499-b1e4-258f3fbc0ea6";
    const transport = createTransport(async () =>
      Response.json(
        { code: "future_private_error", message: "unsafe", requestId },
        { status: 500, headers: { "x-request-id": requestId } },
      ),
    );

    const error = await transport
      .request({ method: "GET", path: "/api/v1/workspace" })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(KernelProtocolError);
    expect(error).toMatchObject({ kind: "invalid-http-response", requestId, status: 500 });
  });

  it("accepts only the frozen authentication rate-limit details", async () => {
    const transport = createTransport(async () =>
      jsonResponse(
        {
          code: "authentication_rate_limited",
          message: "Authentication is temporarily limited.",
          requestId: REQUEST_ID,
          details: { type: "rate-limit", retryAfterSeconds: 31 },
        },
        {
          status: 429,
          headers: { "retry-after": "31", "x-request-id": REQUEST_ID },
        },
      ),
    );

    const error = await transport
      .request({ method: "POST", path: "/api/v1/auth/session", body: {} })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(KernelApiError);
    expect(error).toMatchObject({
      code: "authentication_rate_limited",
      details: { type: "rate-limit", retryAfterSeconds: 31 },
      status: 429,
    });

    for (const retryAfterSeconds of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, "31"]) {
      const invalid = createTransport(async () =>
        jsonResponse(
          {
            code: "authentication_rate_limited",
            message: "Authentication is temporarily limited.",
            requestId: REQUEST_ID,
            details: { type: "rate-limit", retryAfterSeconds },
          },
          {
            status: 429,
            headers: { "retry-after": "31", "x-request-id": REQUEST_ID },
          },
        ),
      );
      await expect(
        invalid.request({ method: "POST", path: "/api/v1/auth/session", body: {} }),
      ).rejects.toBeInstanceOf(KernelProtocolError);
    }

    const invalidRetryHints: Array<{
      details?: { type: string; retryAfterSeconds: number };
      retryAfter?: string;
    }> = [
      { retryAfter: "31" },
      { details: { type: "rate-limit", retryAfterSeconds: 31 } },
      { details: { type: "rate-limit", retryAfterSeconds: 31 }, retryAfter: "30" },
      { details: { type: "rate-limit", retryAfterSeconds: 31 }, retryAfter: "0" },
      { details: { type: "rate-limit", retryAfterSeconds: 31 }, retryAfter: "01" },
      { details: { type: "rate-limit", retryAfterSeconds: 31 }, retryAfter: "+31" },
      { details: { type: "rate-limit", retryAfterSeconds: 31 }, retryAfter: "31.0" },
      { details: { type: "rate-limit", retryAfterSeconds: 31 }, retryAfter: "3 1" },
      {
        details: { type: "rate-limit", retryAfterSeconds: 31 },
        retryAfter: String(Number.MAX_SAFE_INTEGER + 1),
      },
    ];
    for (const { details: invalidDetails, retryAfter } of invalidRetryHints) {
      const invalid = createTransport(async () =>
        jsonResponse(
          {
            code: "authentication_rate_limited",
            message: "Authentication is temporarily limited.",
            requestId: REQUEST_ID,
            ...(invalidDetails === undefined ? {} : { details: invalidDetails }),
          },
          {
            status: 429,
            headers: {
              ...(retryAfter === undefined ? {} : { "retry-after": retryAfter }),
              "x-request-id": REQUEST_ID,
            },
          },
        ),
      );
      await expect(
        invalid.request({ method: "POST", path: "/api/v1/auth/session", body: {} }),
      ).rejects.toBeInstanceOf(KernelProtocolError);
    }

    const maximum = createTransport(async () =>
      jsonResponse(
        {
          code: "authentication_rate_limited",
          message: "Authentication is temporarily limited.",
          requestId: REQUEST_ID,
          details: {
            type: "rate-limit",
            retryAfterSeconds: Number.MAX_SAFE_INTEGER,
          },
        },
        {
          status: 429,
          headers: {
            "retry-after": String(Number.MAX_SAFE_INTEGER),
            "x-request-id": REQUEST_ID,
          },
        },
      ),
    );
    await expect(
      maximum.request({ method: "POST", path: "/api/v1/auth/session", body: {} }),
    ).rejects.toMatchObject({
      code: "authentication_rate_limited",
      details: {
        type: "rate-limit",
        retryAfterSeconds: Number.MAX_SAFE_INTEGER,
      },
    });
  });

  it("rejects unsafe API error messages and mismatched status, details, or validation fields", async () => {
    const requestId = "8a2560cf-88fc-4499-b1e4-258f3fbc0ea6";
    const invalid = [
      { code: "unauthorized", message: `leaked ${SECRET}`, requestId },
      { code: "unauthorized", message: "Authentication is required.", requestId, status: 409 },
      { code: "unauthorized", message: "Authentication is required.", requestId, details: { type: "revision-conflict" } },
      { code: "invalid_request", message: "The request is invalid.", requestId, status: 400, details: { type: "validation", issues: [{ code: "required", field: "privateCredential", message: "This field is required." }] } },
    ];

    for (const candidate of invalid) {
      const { status = 401, ...body } = candidate;
      const transport = createTransport(async () =>
        jsonResponse(body, { status, headers: { "x-request-id": requestId } }),
      );
      const error = await transport.request({ method: "GET", path: "/api/v1/workspace" }).catch((caught: unknown) => caught);
      expect(error).toBeInstanceOf(KernelProtocolError);
      expect(String(error)).not.toContain(SECRET);
    }
  });

  it("never retries a failed mutation and converts aborts to a safe transport error", async () => {
    const controller = new AbortController();
    controller.abort();
    const fetch = vi.fn<FetchLike>(async () => {
      const error = new Error(`network failed near ${SECRET}`);
      error.name = "AbortError";
      throw error;
    });
    const transport = createTransport(fetch);
    const request: HttpRequest = {
      method: "PUT",
      path: "/api/v1/documents/document-1",
      body: {
        workspaceGeneration: "generation-1",
        expectedRevision: "revision-1",
        contents: "updated",
      },
      signal: controller.signal,
    };

    const error = await transport.request(request).catch((caught: unknown) => caught);

    expect(fetch).toHaveBeenCalledTimes(1);
    expect(error).toBeInstanceOf(KernelTransportError);
    expect(error).toMatchObject({ kind: "aborted" });
    expect(String(error)).not.toContain(SECRET);
  });

  it("reads the native bearer for each request instead of retaining a token", async () => {
    const credentials = ["first-credential", "second-credential"];
    const getCredential = vi.fn(() => credentials.shift()!);
    const seen: Array<string | null> = [];
    const transport = new KernelHttpTransport({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async (_url, init) => {
        seen.push(new Headers(init?.headers).get("authorization"));
        return jsonResponse({ apiVersion: "v1", status: "ready", instanceId: "instance-1" });
      },
      auth: { kind: "native-bearer", getCredential },
    });

    await transport.request({ method: "GET", path: "/api/v1/health/ready" });
    await transport.request({ method: "GET", path: "/api/v1/health/ready" });

    expect(getCredential).toHaveBeenCalledTimes(2);
    expect(seen).toEqual(["Bearer first-credential", "Bearer second-credential"]);
  });

  it("rejects authentication variants that Phase 1 does not support", () => {
    expect(
      () =>
        new KernelHttpTransport({
          baseUrl: "http://127.0.0.1:6608",
          fetch: async () => Response.json({}),
          auth: { kind: "cookie-session" } as never,
        }),
    ).toThrow(KernelTransportError);
  });

  it("converts credential provider failures into secret-free transport errors", async () => {
    const transport = new KernelHttpTransport({
      baseUrl: "http://127.0.0.1:6608",
      fetch: async () => Response.json({}),
      auth: {
        kind: "native-bearer",
        getCredential: () => {
          throw new Error(`credential provider leaked ${SECRET}`);
        },
      },
    });

    const error = await transport
      .request({ method: "GET", path: "/api/v1/workspace" })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(KernelTransportError);
    expect(error).toMatchObject({ kind: "credential-unavailable" });
    expect(String(error)).not.toContain(SECRET);
    expect(JSON.stringify(error)).not.toContain(SECRET);
  });

  it("rejects base URLs that could carry ambient credentials, query data, or fragments", () => {
    for (const baseUrl of [
      "http://user:pass@127.0.0.1:6608",
      "http://127.0.0.1:6608?credential=secret",
      "http://127.0.0.1:6608#secret",
      "http://127.0.0.1:6608/nested",
    ]) {
      expect(() =>
        new KernelHttpTransport({
          baseUrl,
          fetch: async () => Response.json({}),
          auth: { kind: "native-bearer", getCredential: () => SECRET },
        }),
      ).toThrow(KernelTransportError);
    }
  });

  it("confines native bearer credentials to the server's exact loopback host", async () => {
    for (const baseUrl of ["http://127.0.0.1:6608"]) {
      const transport = new KernelHttpTransport({
        baseUrl,
        fetch: async () => jsonResponse({}),
        auth: { kind: "native-bearer", getCredential: () => SECRET },
      });
      await expect(
        transport.request({ method: "GET", path: "/api/v1/workspace" }),
      ).resolves.toEqual({});
    }

    for (const baseUrl of [
      "https://attacker.example",
      "https://127.0.0.1:6608",
      "https://127.255.10.20:6608",
      "http://[::1]:6608",
      "https://localhost:6608",
      "http://127.0.0.1",
      "http://127.0.0.1:0",
      "http://foo.localhost:6608",
      "http://localhost.:6608",
      "http://127.1:6608",
      "http://0177.0.0.1:6608",
      "http://2130706433:6608",
    ]) {
      expect(
        () =>
          new KernelHttpTransport({
            baseUrl,
            fetch: async () => Response.json({}),
            auth: { kind: "native-bearer", getCredential: () => SECRET },
          }),
      ).toThrow(KernelTransportError);
    }
  });

  it("accepts every canonical UUID value allowed by the Rust wire contract", async () => {
    const requestId = "00000000-0000-0000-0000-000000000000";
    const transport = createTransport(async () =>
      Response.json(
        {
          code: "unauthorized",
          message: "Authentication is required.",
          requestId,
        },
        { status: 401, headers: { "x-request-id": requestId } },
      ),
    );

    const error = await transport
      .request({ method: "GET", path: "/api/v1/workspace" })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(KernelApiError);
    expect(error).toMatchObject({ requestId });
  });
});

function createTransport(fetch: FetchLike) {
  return new KernelHttpTransport({
    baseUrl: "http://127.0.0.1:6608",
    fetch,
    auth: { kind: "native-bearer", getCredential: () => SECRET },
  });
}

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  const headers = new Headers(init.headers);
  headers.set("x-request-id", REQUEST_ID);
  return Response.json(body, { ...init, headers });
}

function emptyResponse() {
  return new Response(null, {
    status: 204,
    headers: { "x-request-id": REQUEST_ID },
  });
}

function binaryResponse(overrides: Record<string, string>, status = 200) {
  const headers = new Headers({
    "content-length": "11",
    "content-type": "image/png",
    "x-content-type-options": "nosniff",
    "x-request-id": REQUEST_ID,
  });
  for (const [name, value] of Object.entries(overrides)) headers.set(name, value);
  return new Response("image bytes", { status, headers });
}
