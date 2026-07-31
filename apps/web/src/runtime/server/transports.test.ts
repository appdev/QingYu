import type { WebSocketLike } from "@markra/kernel-client";

import { createServerKernelTransports } from "./transports";

describe("Server Kernel browser transports", () => {
  it("connects the authenticated event client to the same-origin WSS endpoint", () => {
    const socket = {
      addEventListener: vi.fn(),
      close: vi.fn(),
      removeEventListener: vi.fn(),
      send: vi.fn(),
    } satisfies WebSocketLike;
    const webSocket = vi.fn(() => socket);
    const transports = createServerKernelTransports({
      browserOrigin: new URL("https://notes.example/"),
      fetch: vi.fn<typeof fetch>(),
      readCookie: () => "__Host-qingyu_csrf=csrf-proof",
      webSocket,
    });

    const connection = transports.events.connect({});

    expect(webSocket).toHaveBeenCalledWith("wss://notes.example/api/v1/events");
    connection.close();
  });

  it("connects HTTP deployments to same-origin API and WS with the HTTP CSRF cookie", async () => {
    const socket = {
      addEventListener: vi.fn(),
      close: vi.fn(),
      removeEventListener: vi.fn(),
      send: vi.fn(),
    } satisfies WebSocketLike;
    const webSocket = vi.fn(() => socket);
    const fetch = vi.fn<typeof globalThis.fetch>(async (_url, init) => {
      expect(new Headers(init?.headers).get("x-csrf-token")).toBe("http-proof");
      return new Response(null, {
        status: 204,
        headers: { "x-request-id": "123e4567-e89b-42d3-a456-426614174000" },
      });
    });
    const transports = createServerKernelTransports({
      browserOrigin: new URL("http://notes.example:3210/"),
      fetch,
      readCookie: () => "qingyu_csrf=http-proof; __Host-qingyu_csrf=https-proof",
      webSocket,
    });

    await transports.client.auth.logout();
    const connection = transports.events.connect({});

    expect(webSocket).toHaveBeenCalledWith("ws://notes.example:3210/api/v1/events");
    connection.close();
  });

  it("rejects a non-origin browser URL instead of normalizing away unsafe components", () => {
    const socket = {
      addEventListener: vi.fn(),
      close: vi.fn(),
      removeEventListener: vi.fn(),
      send: vi.fn(),
    } satisfies WebSocketLike;
    for (const browserOrigin of [
      "https://notes.example/nested",
      "https://notes.example?secret=value",
      "https://notes.example#fragment",
      "https://user:password@notes.example",
    ]) {
      expect(() => createServerKernelTransports({
        browserOrigin,
        fetch: vi.fn<typeof fetch>(),
        readCookie: () => "",
        webSocket: () => socket,
      })).toThrow();
    }
  });
});
