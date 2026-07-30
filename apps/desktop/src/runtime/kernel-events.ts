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
  /**
   * A non-null bootstrap is synchronously consumed and its ownership transfers
   * to this adapter. After handoff, the caller must not release or reuse it in
   * parallel. Initialization failures are released by the adapter. Repassing
   * the exact adopted object keeps the active session, while a different object
   * with the same instance and generation is released immediately. Passing null
   * or calling close releases the adopted ownership.
   */
  replaceConnection(bootstrap: NativeKernelBootstrap | null): undefined;
  close(): undefined;
}

interface ActiveSession {
  readonly identity: DesktopKernelEventsIdentity;
  bootstrap?: NativeKernelBootstrap;
  active: boolean;
  ownershipReleased: boolean;
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
  const releaseOwnership = (session: ActiveSession) => {
    if (session.ownershipReleased) return undefined;
    session.ownershipReleased = true;
    const bootstrap = session.bootstrap;
    session.bootstrap = undefined;
    try {
      bootstrap?.release();
    } catch {
      // Native credential release is best-effort and must never expose provider errors.
    }
    return undefined;
  };
  const retire = (
    session: ActiveSession,
    { closeConnection, notifyClosed }: {
      readonly closeConnection: boolean;
      readonly notifyClosed: boolean;
    }
  ) => {
    if (!session.active) {
      releaseOwnership(session);
      return undefined;
    }
    session.active = false;
    if (current === session) current = undefined;
    if (closeConnection) {
      try {
        session.connection?.close();
      } catch {
        // Connection shutdown is best-effort; ownership release remains mandatory.
      }
    }
    releaseOwnership(session);
    if (notifyClosed) {
      notifyConsumer(options.onStateChange, {
        ...session.identity,
        state: "closed"
      });
    }
    return undefined;
  };
  const replaceConnection = (bootstrap: NativeKernelBootstrap | null) => {
    if (
      bootstrap !== null &&
      current !== undefined &&
      current.identity.instanceId === bootstrap.instanceId &&
      current.identity.generation === bootstrap.generation
    ) {
      if (current.bootstrap === bootstrap) return undefined;
      try {
        bootstrap.release();
      } catch {
        // A redundant bootstrap was not adopted; discard it without surfacing secrets.
      }
      return undefined;
    }

    if (current !== undefined) {
      retire(current, {
        closeConnection: true,
        notifyClosed: bootstrap === null
      });
    }
    if (bootstrap === null) return undefined;

    const identity = Object.freeze({
      instanceId: bootstrap.instanceId,
      generation: bootstrap.generation
    });
    const session: ActiveSession = {
      identity,
      bootstrap,
      active: true,
      ownershipReleased: false
    };
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
          if (!isCurrent(session)) return undefined;
          if (frame.instanceId === session.identity.instanceId) {
            notifyConsumer(options.onStateChange, {
              ...session.identity,
              state: "open"
            });
            return undefined;
          }
          retire(session, { closeConnection: true, notifyClosed: false });
          notifyConsumer(options.onInvalidation, {
            kind: "snapshot-required",
            ...session.identity,
            reason: "instance-mismatch",
            scopes: [...ALL_DOMAIN_SCOPES]
          });
          notifyConsumer(options.onStateChange, {
            ...session.identity,
            state: "closed"
          });
          return undefined;
        },
        onEvent: (frame) => {
          if (!isCurrent(session)) return undefined;
          notifyConsumer(options.onInvalidation, {
            kind: "event",
            ...session.identity,
            scope: domainScopeFor(frame),
            frame
          });
          return undefined;
        },
        onSnapshotRequired: (notice) => {
          if (!isCurrent(session)) return undefined;
          notifyConsumer(options.onInvalidation, {
            kind: "snapshot-required",
            ...session.identity,
            reason: notice.reason,
            scopes: [...notice.reloadScopes]
          });
          return undefined;
        },
        onStateChange: (state) => {
          if (!isCurrent(session)) return undefined;
          if (state === "open") return undefined;
          if (state === "closed") {
            session.active = false;
            current = undefined;
            releaseOwnership(session);
          }
          notifyConsumer(options.onStateChange, { ...session.identity, state });
          return undefined;
        },
        onError: (error) => {
          if (!isCurrent(session)) return undefined;
          notifyConsumer(options.onError, { ...session.identity, error });
          return undefined;
        }
      });
      session.connection = connection;
      if (!isCurrent(session)) {
        try {
          connection.close();
        } catch {
          // A re-entrant replacement already retired this session.
        }
      }
    } catch (error) {
      retire(session, { closeConnection: true, notifyClosed: true });
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

function notifyConsumer<T>(
  consumer: ((notice: T) => unknown) | undefined,
  notice: T
) {
  try {
    consumer?.(notice);
  } catch {
    // Consumer failures are isolated without retaining or exposing their content.
  }
  return undefined;
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
