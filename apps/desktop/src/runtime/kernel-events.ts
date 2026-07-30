import {
  createKernelEventsClient,
  type KernelEventConnection,
  type KernelEventConnectionState,
  type KernelEventError,
  type KernelEventFrame,
  type KernelProtocolError,
  type KernelReloadScope,
  type KernelSnapshotReason,
  type ReconnectScheduler,
  type WebSocketFactory,
  type WebSocketLike
} from "@markra/kernel-client";

import type { NativeKernelBootstrap } from "../kernel-bootstrap";

export type DesktopKernelDomainScope = KernelReloadScope;

export interface DesktopKernelEventsIdentity {
  readonly instanceId: string;
  readonly generation: string;
}

export type DesktopKernelDomainInvalidation =
  | (DesktopKernelEventsIdentity & {
      readonly kind: "event";
      readonly scope: DesktopKernelDomainScope;
      readonly frame: KernelEventFrame;
    })
  | (DesktopKernelEventsIdentity & {
      readonly kind: "snapshot-required";
      readonly reason: KernelSnapshotReason | "instance-mismatch";
      readonly scopes: readonly DesktopKernelDomainScope[];
    });

export type DesktopKernelEventsStateNotice = DesktopKernelEventsIdentity & {
  readonly state: KernelEventConnectionState;
};

export type DesktopKernelEventsErrorNotice = DesktopKernelEventsIdentity & {
  readonly error: KernelEventError | KernelProtocolError;
};

export interface DesktopKernelEventsAdapterOptions {
  readonly onInvalidation: (invalidation: DesktopKernelDomainInvalidation) => unknown;
  readonly onStateChange?: (notice: DesktopKernelEventsStateNotice) => unknown;
  readonly onError?: (notice: DesktopKernelEventsErrorNotice) => unknown;
  readonly webSocket?: WebSocketFactory;
  readonly scheduleReconnect?: ReconnectScheduler;
  readonly reconnectDelayMs?: number;
}

export interface DesktopKernelEventsAdapter {
  readonly identity: DesktopKernelEventsIdentity | null;
  replaceConnection(bootstrap: NativeKernelBootstrap | null): undefined;
  close(): undefined;
}

interface ActiveSession {
  readonly identity: DesktopKernelEventsIdentity;
  active: boolean;
  connection?: KernelEventConnection;
}

const ALL_DOMAIN_SCOPES = [
  "workspace",
  "documents",
  "settings",
  "sync-config",
  "sync-status"
] as const satisfies readonly DesktopKernelDomainScope[];

const createBrowserWebSocket: WebSocketFactory = (url) => (
  new WebSocket(url) as unknown as WebSocketLike
);

export function createDesktopKernelEventsAdapter(
  options: DesktopKernelEventsAdapterOptions
): DesktopKernelEventsAdapter {
  let current: ActiveSession | undefined;

  const isCurrent = (session: ActiveSession) => session.active && current === session;
  const stop = (session: ActiveSession) => {
    if (!session.active) return undefined;
    session.active = false;
    if (current === session) current = undefined;
    session.connection?.close();
    return undefined;
  };
  const replaceConnection = (bootstrap: NativeKernelBootstrap | null) => {
    if (
      bootstrap !== null &&
      current !== undefined &&
      current.identity.instanceId === bootstrap.instanceId &&
      current.identity.generation === bootstrap.generation
    ) {
      return undefined;
    }

    if (current !== undefined) stop(current);
    if (bootstrap === null) return undefined;

    const identity = Object.freeze({
      instanceId: bootstrap.instanceId,
      generation: bootstrap.generation
    });
    const session: ActiveSession = { identity, active: true };
    current = session;

    try {
      const client = createKernelEventsClient({
        baseUrl: bootstrap.baseUrl,
        auth: bootstrap.authentication,
        webSocket: options.webSocket ?? createBrowserWebSocket,
        scheduleReconnect: options.scheduleReconnect,
        reconnectDelayMs: options.reconnectDelayMs
      });
      const connection = client.connect({
        onReady: (frame) => {
          if (!isCurrent(session) || frame.instanceId === session.identity.instanceId) {
            return undefined;
          }
          stop(session);
          options.onInvalidation({
            kind: "snapshot-required",
            ...session.identity,
            reason: "instance-mismatch",
            scopes: [...ALL_DOMAIN_SCOPES]
          });
          return undefined;
        },
        onEvent: (frame) => {
          if (!isCurrent(session)) return undefined;
          options.onInvalidation({
            kind: "event",
            ...session.identity,
            scope: domainScopeFor(frame),
            frame
          });
          return undefined;
        },
        onSnapshotRequired: (notice) => {
          if (!isCurrent(session)) return undefined;
          options.onInvalidation({
            kind: "snapshot-required",
            ...session.identity,
            reason: notice.reason,
            scopes: [...notice.reloadScopes]
          });
          return undefined;
        },
        onStateChange: (state) => {
          if (!isCurrent(session)) return undefined;
          if (state === "closed") {
            session.active = false;
            current = undefined;
          }
          options.onStateChange?.({ ...session.identity, state });
          return undefined;
        },
        onError: (error) => {
          if (!isCurrent(session)) return undefined;
          options.onError?.({ ...session.identity, error });
          return undefined;
        }
      });
      session.connection = connection;
      if (!isCurrent(session)) connection.close();
    } catch (error) {
      stop(session);
      throw error;
    }
    return undefined;
  };

  return {
    get identity() {
      return current?.identity ?? null;
    },
    replaceConnection,
    close: () => replaceConnection(null)
  };
}

function domainScopeFor(frame: KernelEventFrame): DesktopKernelDomainScope {
  switch (frame.event.type) {
    case "workspace-changed":
      return "workspace";
    case "document-created":
    case "document-changed":
    case "document-moved":
    case "document-deleted":
      return "documents";
    case "settings-changed":
      return "settings";
    case "sync-config-changed":
      return "sync-config";
    case "sync-status-changed":
      return "sync-status";
  }
}
