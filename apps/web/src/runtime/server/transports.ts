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
  const browserOrigin = new URL("/", options.browserOrigin);
  const authentication = {
    browserOrigin,
    getCsrfToken: () => readServerCsrfCookie(options.readCookie()),
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
