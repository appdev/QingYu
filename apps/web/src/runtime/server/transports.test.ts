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
});
