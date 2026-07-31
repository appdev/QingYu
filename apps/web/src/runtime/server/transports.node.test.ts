// @vitest-environment node

import type { WebSocketLike } from "@markra/kernel-client";

import { createServerKernelTransports } from "./transports";

describe("Server Kernel malformed origin errors in Node", () => {
  it("does not retain the rejected origin in string or serialized error fields", () => {
    const socket = {
      addEventListener: vi.fn(),
      close: vi.fn(),
      removeEventListener: vi.fn(),
      send: vi.fn(),
    } satisfies WebSocketLike;
    const sentinel = "sentinel-node-browser-origin-secret";
    let caught: unknown;

    try {
      createServerKernelTransports({
        browserOrigin: `https://user:${sentinel}@[`,
        fetch: vi.fn<typeof fetch>(),
        readCookie: () => "",
        webSocket: () => socket,
      });
    } catch (error: unknown) {
      caught = error;
    }

    expect(caught).toBeInstanceOf(Error);
    expect(String(caught)).toBe("Error: invalid-base-url");
    expect(String(caught)).not.toContain(sentinel);
    expect(JSON.stringify(caught)).not.toContain(sentinel);
  });
});
