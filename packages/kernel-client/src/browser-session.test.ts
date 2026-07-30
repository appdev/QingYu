import { describe, expect, it, vi } from "vitest";

import {
  createKernelClient,
  KernelProtocolError,
  KernelTransportError,
  type FetchLike,
  type HttpRequest,
} from "./index.ts";
import { KernelHttpTransport } from "./transport.ts";

const BASE_URL = "https://notes.example:8443";
const BROWSER_ORIGIN = "https://notes.example:8443";
const CSRF_SECRET = "csrf-secret-that-must-not-leak";
const PASSWORD_SECRET = "password-secret-that-must-not-leak";
const INITIALIZATION_SECRET = "initialization-secret-that-must-not-leak";
const REQUEST_ID = "123e4567-e89b-42d3-a456-426614174000";

describe("browser session transport", () => {
  it("uses only same-origin cookies for HTTPS reads", async () => {
    const getCsrfToken = vi.fn(() => CSRF_SECRET);
    const fetch = vi.fn<FetchLike>(async (url, init) => {
      expect(url).toBe(`${BASE_URL}/api/v1/workspace`);
      expect(init?.credentials).toBe("same-origin");
      expect(new Headers(init?.headers).has("authorization")).toBe(false);
      expect(new Headers(init?.headers).has("x-csrf-token")).toBe(false);
      return jsonResponse({});
    });
    const transport = browserTransport(fetch, getCsrfToken);

    await transport.request({ method: "GET", path: "/api/v1/workspace" });

    expect(fetch).toHaveBeenCalledTimes(1);
    expect(getCsrfToken).not.toHaveBeenCalled();
  });

  it("does not require an existing CSRF token for initialization or login", async () => {
    const getCsrfToken = vi.fn(() => null);
    const calls: RequestInit[] = [];
    const transport = browserTransport(async (_url, init = {}) => {
      calls.push(init);
      return jsonResponse({ state: "authenticated" }, { status: 201 });
    }, getCsrfToken);

    await transport.request({
      method: "POST",
      path: "/api/v1/auth/initialize",
      body: { initializationToken: INITIALIZATION_SECRET, password: PASSWORD_SECRET },
    });
    await transport.request({
      method: "POST",
      path: "/api/v1/auth/session",
      body: { password: PASSWORD_SECRET },
    });

    expect(getCsrfToken).not.toHaveBeenCalled();
    expect(calls).toHaveLength(2);
    for (const init of calls) {
      expect(init.credentials).toBe("same-origin");
      expect(new Headers(init.headers).has("authorization")).toBe(false);
      expect(new Headers(init.headers).has("x-csrf-token")).toBe(false);
    }
  });

  it("adds a fresh CSRF proof to every protected mutation", async () => {
    const csrfTokens = Array.from({ length: 11 }, (_, index) => `csrf-${index + 1}`);
    const getCsrfToken = vi.fn(() => csrfTokens.shift());
    const calls: Array<{ url: URL; init: RequestInit }> = [];
    const transport = browserTransport(async (url, init = {}) => {
      calls.push({ url: new URL(url), init });
      return emptyResponse();
    }, getCsrfToken);
    const mutations: HttpRequest[] = [
      { method: "POST", path: "/api/v1/auth/logout" },
      { method: "PATCH", path: "/api/v1/auth/password", body: {} },
      { method: "POST", path: "/api/v1/documents", body: {} },
      { method: "PUT", path: "/api/v1/documents/note.md", body: {} },
      { method: "POST", path: "/api/v1/documents/note.md/move", body: {} },
      { method: "POST", path: "/api/v1/documents/note.md/delete", body: {} },
      { method: "POST", path: "/api/v1/documents/note.md/history/snapshot-1/restore", body: {} },
      { method: "PATCH", path: "/api/v1/settings", body: {} },
      { method: "PATCH", path: "/api/v1/sync/config", body: {} },
      { method: "POST", path: "/api/v1/sync/connection-test", body: {} },
      { method: "POST", path: "/api/v1/sync/runs", body: {} },
    ];

    for (const mutation of mutations) await transport.request(mutation);

    expect(getCsrfToken).toHaveBeenCalledTimes(mutations.length);
    expect(calls.map(({ init }) => new Headers(init.headers).get("x-csrf-token"))).toEqual(
      Array.from({ length: mutations.length }, (_, index) => `csrf-${index + 1}`),
    );
    expect(calls.every(({ init }) => init.credentials === "same-origin")).toBe(true);
    expect(calls.every(({ init }) => !new Headers(init.headers).has("authorization"))).toBe(true);
  });

  it("fails closed before fetch when the CSRF proof is missing, unsafe, or unavailable", async () => {
    for (const getCsrfToken of [
      () => undefined,
      () => null,
      () => "",
      () => "   ",
      () => "unsafe\ncsrf",
      () => "csrf-秘密",
      () => {
        throw new Error(`provider leaked ${CSRF_SECRET}`);
      },
    ]) {
      const fetch = vi.fn<FetchLike>();
      const transport = browserTransport(fetch, getCsrfToken);
      const error = await transport
        .request({ method: "PATCH", path: "/api/v1/settings", body: {} })
        .catch((caught: unknown) => caught);

      expect(fetch).not.toHaveBeenCalled();
      expect(error).toBeInstanceOf(KernelTransportError);
      expect(error).toMatchObject({ kind: "csrf-unavailable" });
      expect(String(error)).not.toContain(CSRF_SECRET);
      expect(JSON.stringify(error)).not.toContain(CSRF_SECRET);
    }
  });

  it("rejects insecure, cross-origin, and non-root browser endpoints", () => {
    const invalidPairs = [
      ["http://notes.example", "http://notes.example"],
      ["https://api.example", "https://notes.example"],
      ["https://notes.example/nested", "https://notes.example"],
      ["https://notes.example?secret=x", "https://notes.example"],
      ["https://notes.example#secret", "https://notes.example"],
      ["https://user:pass@notes.example", "https://notes.example"],
      ["https://notes.example", "http://notes.example"],
      ["https://notes.example", "https://notes.example/nested"],
    ] as const;

    for (const [baseUrl, browserOrigin] of invalidPairs) {
      expect(() => browserTransport(async () => jsonResponse({}), () => CSRF_SECRET, {
        baseUrl,
        browserOrigin,
      })).toThrow(KernelTransportError);
    }
  });

  it("rejects caller credential overrides before issuing a browser request", async () => {
    const fetch = vi.fn<FetchLike>();
    const transport = browserTransport(fetch, () => CSRF_SECRET);
    const request = {
      method: "GET",
      path: "/api/v1/workspace",
      credentials: "include",
    } as HttpRequest;

    await expect(transport.request(request)).rejects.toMatchObject({ kind: "invalid-request" });
    expect(fetch).not.toHaveBeenCalled();
  });

  it("snapshots the validated browser authentication mode at construction", async () => {
    const auth: Record<string, unknown> = {
      kind: "browser-session",
      browserOrigin: BROWSER_ORIGIN,
      getCsrfToken: () => CSRF_SECRET,
    };
    const fetch = vi.fn<FetchLike>(async (_url, init) => {
      expect(init?.credentials).toBe("same-origin");
      expect(new Headers(init?.headers).get("authorization")).toBeNull();
      expect(new Headers(init?.headers).get("x-csrf-token")).toBe(CSRF_SECRET);
      return emptyResponse();
    });
    const transport = new KernelHttpTransport({
      baseUrl: BASE_URL,
      fetch,
      auth: auth as never,
    });
    auth.kind = "native-bearer";
    auth.getCredential = () => PASSWORD_SECRET;

    await transport.request({ method: "PATCH", path: "/api/v1/settings", body: {} });

    expect(fetch).toHaveBeenCalledTimes(1);
  });
});

describe("browser session auth client", () => {
  it("maps every auth method to its exact server route and status", async () => {
    const calls: Array<{ url: URL; init: RequestInit }> = [];
    const client = createKernelClient({
      baseUrl: BASE_URL,
      fetch: async (url, init = {}) => {
        calls.push({ url: new URL(url), init });
        const path = new URL(url).pathname;
        if (path === "/api/v1/auth/status") {
          return jsonResponse({ initialization: "required" });
        }
        if (path === "/api/v1/auth/session" && init.method === "GET") {
          return jsonResponse({ state: "authenticated" });
        }
        if (path === "/api/v1/auth/logout" || path === "/api/v1/auth/password") {
          return emptyResponse();
        }
        return jsonResponse({ state: "authenticated" }, { status: 201 });
      },
      auth: {
        kind: "browser-session",
        browserOrigin: BROWSER_ORIGIN,
        getCsrfToken: () => CSRF_SECRET,
      },
    });
    const signal = new AbortController().signal;

    await client.auth.status({ signal });
    await client.auth.initialize({
      initializationToken: INITIALIZATION_SECRET,
      password: PASSWORD_SECRET,
    }, { signal });
    await client.auth.login({ password: PASSWORD_SECRET }, { signal });
    await client.auth.getSession({ signal });
    await client.auth.logout({ signal });
    await client.auth.changePassword({
      currentPassword: PASSWORD_SECRET,
      newPassword: `${PASSWORD_SECRET}-new`,
    }, { signal });

    expect(calls.map(({ url, init }) => `${init.method} ${url.pathname}`)).toEqual([
      "GET /api/v1/auth/status",
      "POST /api/v1/auth/initialize",
      "POST /api/v1/auth/session",
      "GET /api/v1/auth/session",
      "POST /api/v1/auth/logout",
      "PATCH /api/v1/auth/password",
    ]);
    expect(calls.every(({ init }) => init.signal === signal)).toBe(true);
    expect(new Headers(calls[1]?.init.headers).has("x-csrf-token")).toBe(false);
    expect(new Headers(calls[2]?.init.headers).has("x-csrf-token")).toBe(false);
    expect(new Headers(calls[4]?.init.headers).get("x-csrf-token")).toBe(CSRF_SECRET);
    expect(new Headers(calls[5]?.init.headers).get("x-csrf-token")).toBe(CSRF_SECRET);
  });

  it("strictly validates auth response bodies and success statuses without leaking secrets", async () => {
    const invalidBodies = [
      { initialization: "future" },
      { initialization: "required", extra: true },
      {},
    ];
    for (const body of invalidBodies) {
      const client = browserClient(async () => jsonResponse(body));
      await expect(client.auth.status()).rejects.toBeInstanceOf(KernelProtocolError);
    }

    const malformedSession = browserClient(async () =>
      jsonResponse({ state: "authenticated", secret: PASSWORD_SECRET }, { status: 201 }),
    );
    const malformedError = await malformedSession.auth.login({ password: PASSWORD_SECRET })
      .catch((caught: unknown) => caught);
    expect(malformedError).toBeInstanceOf(KernelProtocolError);
    expect(String(malformedError)).not.toContain(PASSWORD_SECRET);
    expect(JSON.stringify(malformedError)).not.toContain(PASSWORD_SECRET);

    const wrongStatus = browserClient(async () =>
      jsonResponse({ state: "authenticated" }, { status: 200 }),
    );
    await expect(wrongStatus.auth.login({ password: PASSWORD_SECRET }))
      .rejects.toBeInstanceOf(KernelProtocolError);
  });
});

function browserTransport(
  fetch: FetchLike,
  getCsrfToken: () => string | null | undefined,
  endpoints: { baseUrl?: string; browserOrigin?: string } = {},
) {
  return new KernelHttpTransport({
    baseUrl: endpoints.baseUrl ?? BASE_URL,
    fetch,
    auth: {
      kind: "browser-session",
      browserOrigin: endpoints.browserOrigin ?? BROWSER_ORIGIN,
      getCsrfToken,
    },
  });
}

function browserClient(fetch: FetchLike) {
  return createKernelClient({
    baseUrl: BASE_URL,
    fetch,
    auth: {
      kind: "browser-session",
      browserOrigin: BROWSER_ORIGIN,
      getCsrfToken: () => CSRF_SECRET,
    },
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
