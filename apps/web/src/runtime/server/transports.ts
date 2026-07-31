import {
  createKernelClient,
  createKernelEventsClient,
  type FetchLike,
  type KernelAuthentication,
  type WebSocketFactory,
} from "@markra/kernel-client";

import { readServerCsrfCookie } from "./csrf";

export interface ServerKernelTransportOptions {
  readonly browserOrigin: string | URL;
  readonly fetch: FetchLike;
  readonly readCookie: () => string;
  readonly webSocket: WebSocketFactory;
}

export function createServerKernelTransports(options: ServerKernelTransportOptions) {
  const browserOrigin = parseBrowserOrigin(options.browserOrigin);
  const authentication = {
    browserOrigin,
    getCsrfToken: () => readServerCsrfCookie(options.readCookie(), browserOrigin),
    kind: "browser-session",
  } satisfies KernelAuthentication;

  return {
    client: createKernelClient({
      auth: authentication,
      baseUrl: browserOrigin,
      fetch: options.fetch,
    }),
    events: createKernelEventsClient({
      auth: authentication,
      baseUrl: browserOrigin,
      webSocket: options.webSocket,
    }),
  };
}

function parseBrowserOrigin(value: string | URL): URL {
  try {
    return new URL(value);
  } catch {
    throw new Error("invalid-base-url");
  }
}
