import type { components } from "./generated/kernel-v1.ts";
import { isRfc3339Utc } from "./datetime.ts";
import {
  KernelEventError,
  KernelProtocolError,
  type KernelEventErrorKind,
  type KernelProtocolErrorKind,
} from "./errors.ts";
import {
  parseKernelBaseUrl,
  type KernelAuthentication,
} from "./transport.ts";

type Schemas = components["schemas"];

export type KernelReadyFrame = Schemas["ReadyFrame"];
export type KernelEventFrame = Schemas["EventFrame"];
export type KernelReloadScope = Schemas["ReloadScope"];

export type WebSocketEventType = "open" | "message" | "close" | "error";

export interface WebSocketEvent {
  data?: unknown;
  code?: number;
  reason?: string;
  wasClean?: boolean;
}

export interface WebSocketLike {
  addEventListener(
    type: WebSocketEventType,
    listener: (event: WebSocketEvent) => unknown,
  ): unknown;
  removeEventListener(
    type: WebSocketEventType,
    listener: (event: WebSocketEvent) => unknown,
  ): unknown;
  send(data: string): unknown;
  close(code?: number, reason?: string): unknown;
}

export type WebSocketFactory = (url: string) => WebSocketLike;
export type ReconnectScheduler = (
  callback: () => unknown,
  delayMs: number,
) => () => unknown;

export type KernelEventConnectionState =
  | "connecting"
  | "open"
  | "stale"
  | "reconnecting"
  | "closed";

export type KernelSnapshotReason =
  | "ready"
  | "reconnect"
  | "sequence-gap"
  | "connection-mismatch"
  | "server-gap";

export interface KernelSnapshotNotice {
  reason: KernelSnapshotReason;
  reloadScopes: KernelReloadScope[];
}

export interface KernelEventHandlers {
  onReady?: (frame: KernelReadyFrame) => unknown;
  onEvent?: (frame: KernelEventFrame) => unknown;
  onSnapshotRequired?: (notice: KernelSnapshotNotice) => unknown;
  onStateChange?: (state: KernelEventConnectionState) => unknown;
  onError?: (error: KernelEventError | KernelProtocolError) => unknown;
}

export interface KernelEventConnectOptions {
  signal?: AbortSignal;
}

export interface KernelEventConnection {
  readonly state: KernelEventConnectionState;
  close(): unknown;
}

export interface KernelEventsClientOptions {
  baseUrl: string | URL;
  auth: KernelAuthentication;
  webSocket: WebSocketFactory;
  scheduleReconnect?: ReconnectScheduler;
  reconnectDelayMs?: number;
}

export interface KernelEventsClient {
  connect(
    handlers: KernelEventHandlers,
    options?: KernelEventConnectOptions,
  ): KernelEventConnection;
}

export type KernelEventStream = KernelEventsClient;

const PROTOCOL_VERSION = 1;
const AUTHENTICATION_CLOSE_CODE = 4001;
const INVALID_FRAME_CLOSE_CODE = 4002;
const RELOAD_CLOSE_CODE = 4009;
const ALL_RELOAD_SCOPES: KernelReloadScope[] = [
  "workspace",
  "documents",
  "settings",
  "sync-config",
  "sync-status",
];

const defaultScheduleReconnect: ReconnectScheduler = (callback, delayMs) => {
  const handle = setTimeout(callback, delayMs);
  return () => clearTimeout(handle);
};

export function createKernelEventsClient(
  options: KernelEventsClientOptions,
): KernelEventsClient {
  const baseUrl = parseKernelBaseUrl(options.baseUrl);
  if (options.auth?.kind !== "native-bearer" || typeof options.auth.getCredential !== "function") {
    throw new KernelEventError("connection");
  }
  const eventsUrl = new URL(baseUrl);
  eventsUrl.protocol = eventsUrl.protocol === "https:" ? "wss:" : "ws:";
  eventsUrl.pathname = "/api/v1/events";
  const scheduleReconnect = options.scheduleReconnect ?? defaultScheduleReconnect;
  const reconnectDelayMs = options.reconnectDelayMs ?? 250;

  return {
    connect: (handlers, connectOptions) =>
      connectEvents({
        url: eventsUrl.toString(),
        auth: options.auth,
        webSocket: options.webSocket,
        scheduleReconnect,
        reconnectDelayMs,
        handlers,
        signal: connectOptions?.signal,
      }),
  };
}

interface ActiveConnectionOptions {
  url: string;
  auth: KernelAuthentication;
  webSocket: WebSocketFactory;
  scheduleReconnect: ReconnectScheduler;
  reconnectDelayMs: number;
  handlers: KernelEventHandlers;
  signal?: AbortSignal;
}

function connectEvents(options: ActiveConnectionOptions): KernelEventConnection {
  let state: KernelEventConnectionState = "connecting";
  let socket: WebSocketLike | undefined;
  let cancelReconnect: (() => unknown) | undefined;
  let detachSocket: (() => unknown) | undefined;
  let stopped = false;
  let terminal = false;
  let attempt = 0;
  let phase: "awaiting-ready" | "ready" | "stale" = "awaiting-ready";
  let connectionId: string | undefined;
  let expectedSequence = 1;

  const setState = (next: KernelEventConnectionState) => {
    state = next;
    options.handlers.onStateChange?.(next);
  };

  const closeSocket = (code: number, reason: string) => {
    try {
      socket?.close(code, reason);
    } catch {
      options.handlers.onError?.(new KernelEventError("connection"));
    }
  };

  const terminate = (
    error: KernelEventError | KernelProtocolError,
    closeCode: number,
  ) => {
    if (stopped || terminal) return;
    terminal = true;
    options.handlers.onError?.(error);
    closeSocket(closeCode, "event protocol failed");
  };

  const failProtocol = (kind: KernelProtocolErrorKind) => {
    terminate(
      new KernelProtocolError(kind),
      kind === "unsupported-websocket-version"
        ? AUTHENTICATION_CLOSE_CODE
        : INVALID_FRAME_CLOSE_CODE,
    );
  };

  const failEvent = (kind: KernelEventErrorKind, frameCode?: string) => {
    terminate(
      new KernelEventError(kind, { frameCode }),
      kind === "server-error" ? AUTHENTICATION_CLOSE_CODE : INVALID_FRAME_CLOSE_CODE,
    );
  };

  const requireSnapshot = (
    reason: KernelSnapshotReason,
    reloadScopes: KernelReloadScope[],
  ) => {
    if (stopped || terminal || phase === "stale") return;
    phase = "stale";
    setState("stale");
    options.handlers.onSnapshotRequired?.({ reason, reloadScopes: [...reloadScopes] });
    closeSocket(RELOAD_CLOSE_CODE, "snapshot reload required");
  };

  const handleFrame = (raw: unknown) => {
    if (stopped || terminal || phase === "stale") return;
    if (typeof raw !== "string") {
      failProtocol("invalid-websocket-frame");
      return;
    }
    let frame: unknown;
    try {
      frame = JSON.parse(raw);
    } catch {
      failProtocol("invalid-websocket-frame");
      return;
    }
    if (!isRecord(frame)) {
      failProtocol("invalid-websocket-frame");
      return;
    }
    if (typeof frame.protocolVersion !== "number") {
      failProtocol("invalid-websocket-frame");
      return;
    }
    if (frame.protocolVersion !== PROTOCOL_VERSION) {
      failProtocol("unsupported-websocket-version");
      return;
    }
    if (frame.type === "error") {
      if (!isErrorFrame(frame)) {
        failProtocol("invalid-websocket-frame");
        return;
      }
      failEvent("server-error", frame.code);
      return;
    }

    if (phase === "awaiting-ready") {
      if (!isReadyFrame(frame)) {
        failProtocol("invalid-websocket-frame");
        return;
      }
      phase = "ready";
      connectionId = frame.connectionId;
      expectedSequence = 1;
      setState("open");
      options.handlers.onReady?.(frame);
      options.handlers.onSnapshotRequired?.({
        reason: attempt === 1 ? "ready" : "reconnect",
        reloadScopes: [...ALL_RELOAD_SCOPES],
      });
      return;
    }

    if (frame.type === "event") {
      if (!isEventFrame(frame)) {
        failProtocol("invalid-websocket-frame");
        return;
      }
      if (frame.connectionId !== connectionId) {
        requireSnapshot("connection-mismatch", ALL_RELOAD_SCOPES);
        return;
      }
      if (frame.sequence < expectedSequence) return;
      if (frame.sequence > expectedSequence) {
        requireSnapshot("sequence-gap", ALL_RELOAD_SCOPES);
        return;
      }
      expectedSequence = frame.sequence + 1;
      options.handlers.onEvent?.(frame);
      return;
    }

    if (frame.type === "gap") {
      if (!isGapFrame(frame)) {
        failProtocol("invalid-websocket-frame");
        return;
      }
      if (frame.connectionId !== connectionId) {
        requireSnapshot("connection-mismatch", ALL_RELOAD_SCOPES);
        return;
      }
      requireSnapshot("server-gap", frame.reloadScopes);
      return;
    }

    failProtocol("invalid-websocket-frame");
  };

  const connectSocket = () => {
    if (stopped || terminal) return;
    attempt += 1;
    phase = "awaiting-ready";
    connectionId = undefined;
    expectedSequence = 1;
    setState("connecting");

    let nextSocket: WebSocketLike;
    try {
      nextSocket = options.webSocket(options.url);
    } catch {
      options.handlers.onError?.(new KernelEventError("connection"));
      scheduleNext();
      return;
    }
    socket = nextSocket;

    const onOpen = () => {
      if (stopped || terminal || socket !== nextSocket) return;
      try {
        const credential = options.auth.getCredential();
        nextSocket.send(
          JSON.stringify({
            type: "authenticate",
            protocolVersion: PROTOCOL_VERSION,
            credential,
          }),
        );
      } catch {
        failEvent("connection");
      }
    };
    const onMessage = (event: WebSocketEvent) => {
      if (stopped || terminal || socket !== nextSocket) return;
      handleFrame(event.data);
    };
    const onClose = (event: WebSocketEvent) => {
      if (socket !== nextSocket) return;
      detachSocket?.();
      detachSocket = undefined;
      socket = undefined;
      if (stopped || terminal) {
        setState("closed");
        return;
      }
      if (event.code === AUTHENTICATION_CLOSE_CODE) {
        terminal = true;
        options.handlers.onError?.(new KernelEventError("server-error", { frameCode: "unauthorized" }));
        setState("closed");
        return;
      }
      scheduleNext();
    };
    const onError = () => {
      if (stopped || terminal || socket !== nextSocket) return;
      options.handlers.onError?.(new KernelEventError("connection"));
      closeSocket(INVALID_FRAME_CLOSE_CODE, "event connection failed");
    };

    nextSocket.addEventListener("open", onOpen);
    nextSocket.addEventListener("message", onMessage);
    nextSocket.addEventListener("close", onClose);
    nextSocket.addEventListener("error", onError);
    detachSocket = () => {
      nextSocket.removeEventListener("open", onOpen);
      nextSocket.removeEventListener("message", onMessage);
      nextSocket.removeEventListener("close", onClose);
      nextSocket.removeEventListener("error", onError);
    };
  };

  const stop = () => {
    if (stopped) return;
    stopped = true;
    cancelReconnect?.();
    cancelReconnect = undefined;
    options.signal?.removeEventListener("abort", stop);
    closeSocket(1000, "client closed");
    detachSocket?.();
    detachSocket = undefined;
    socket = undefined;
    setState("closed");
  };

  if (options.signal?.aborted === true) {
    stopped = true;
    setState("closed");
  } else {
    options.signal?.addEventListener("abort", stop, { once: true });
    connectSocket();
  }

  return {
    get state() {
      return state;
    },
    close: stop,
  };

  function scheduleNext() {
    if (stopped || terminal || cancelReconnect !== undefined) return;
    setState("reconnecting");
    cancelReconnect = options.scheduleReconnect(() => {
      cancelReconnect = undefined;
      connectSocket();
    }, options.reconnectDelayMs);
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isReadyFrame(frame: Record<string, unknown>): frame is KernelReadyFrame {
  return (
    frame.type === "ready" &&
    frame.protocolVersion === PROTOCOL_VERSION &&
    isUuid(frame.connectionId) &&
    isUuid(frame.instanceId) &&
    frame.sequence === 0 &&
    frame.snapshotRequired === true &&
    hasExactKeys(frame, [
      "type",
      "protocolVersion",
      "connectionId",
      "instanceId",
      "sequence",
      "snapshotRequired",
    ])
  );
}

function isEventFrame(frame: Record<string, unknown>): frame is KernelEventFrame {
  return (
    frame.type === "event" &&
    frame.protocolVersion === PROTOCOL_VERSION &&
    isUuid(frame.connectionId) &&
    isEventSequence(frame.sequence) &&
    isRevision(frame.revision) &&
    isResourceRef(frame.resource) &&
    isDomainEvent(frame.event) &&
    isEventRelationship(frame.resource, frame.revision, frame.event) &&
    hasExactKeys(frame, [
      "type",
      "protocolVersion",
      "connectionId",
      "sequence",
      "resource",
      "revision",
      "event",
    ])
  );
}

function isEventRelationship(
  resource: Schemas["ResourceRefDto"],
  revision: string,
  event: Schemas["DomainEvent"],
) {
  switch (event.type) {
    case "workspace-changed":
      return resource.kind === "workspace" && resource.id === event.workspace.id && revision === event.workspace.revision;
    case "document-created":
    case "document-changed":
    case "document-moved":
      return resource.kind === "document" && resource.id === event.document.id && revision === event.document.revision;
    case "document-deleted":
      return resource.kind === "document" && resource.id === event.documentId && revision === event.revision;
    case "settings-changed":
      return resource.kind === "settings" && revision === event.settings.revision;
    case "sync-config-changed":
      return resource.kind === "sync-config" && revision === event.config.revision;
    case "sync-status-changed": {
      if (
        resource.kind !== "sync-status" ||
        (event.status.configRevision !== null && revision !== event.status.configRevision)
      ) {
        return false;
      }
      if (event.status.completionState === "idle") {
        return resource.runId === null && event.status.activeRunId === null;
      }
      if (event.status.completionState === "attempting") {
        return (
          event.status.activeRunId !== null &&
          resource.runId === event.status.activeRunId
        );
      }
      if (event.status.activeRunId !== null || resource.runId === null) {
        return false;
      }
      return (
        event.status.error === null ||
        event.status.error.runId === undefined ||
        event.status.error.runId === resource.runId
      );
    }
  }
}

function isGapFrame(
  frame: Record<string, unknown>,
): frame is Extract<Schemas["ServerFrame"], { type: "gap" }> {
  return (
    frame.type === "gap" &&
    frame.protocolVersion === PROTOCOL_VERSION &&
    isUuid(frame.connectionId) &&
    isEventSequence(frame.sequence) &&
    (frame.reason === "buffer-overflow" || frame.reason === "sequence-exhausted") &&
    Array.isArray(frame.reloadScopes) &&
    frame.reloadScopes.every(isReloadScope) &&
    hasExactKeys(frame, [
      "type",
      "protocolVersion",
      "connectionId",
      "sequence",
      "reason",
      "reloadScopes",
    ])
  );
}

function isErrorFrame(
  frame: Record<string, unknown>,
): frame is Extract<Schemas["ServerFrame"], { type: "error" }> {
  return (
    frame.type === "error" &&
    frame.protocolVersion === PROTOCOL_VERSION &&
    (frame.code === "unauthorized" ||
      frame.code === "invalid-frame" ||
      frame.code === "unsupported-version") &&
    typeof frame.message === "string" &&
    hasExactKeys(frame, ["type", "protocolVersion", "code", "message"])
  );
}

function isEventSequence(value: unknown) {
  return Number.isSafeInteger(value) && typeof value === "number" && value > 0;
}

function isReloadScope(value: unknown): value is KernelReloadScope {
  return ALL_RELOAD_SCOPES.some((scope) => scope === value);
}

function isResourceRef(value: unknown): value is Schemas["ResourceRefDto"] {
  if (!isRecord(value)) return false;
  switch (value.kind) {
    case "workspace":
      return isUuid(value.id) && hasExactKeys(value, ["kind", "id"]);
    case "document":
      return isDocumentId(value.id) && hasExactKeys(value, ["kind", "id"]);
    case "settings":
    case "sync-config":
      return hasExactKeys(value, ["kind"]);
    case "sync-status":
      return (
        (value.runId === null || isUuid(value.runId)) &&
        hasExactKeys(value, ["kind", "runId"])
      );
    default:
      return false;
  }
}

function isDomainEvent(value: unknown): value is Schemas["DomainEvent"] {
  if (!isRecord(value)) return false;
  switch (value.type) {
    case "workspace-changed":
      return isWorkspace(value.workspace) && hasExactKeys(value, ["type", "workspace"]);
    case "document-created":
    case "document-changed":
      return isDocumentEntry(value.document) && hasExactKeys(value, ["type", "document"]);
    case "document-moved":
      return (
        isDocumentEntry(value.document) &&
        isWorkspaceRelativePath(value.previousPath) &&
        hasExactKeys(value, ["type", "document", "previousPath"])
      );
    case "document-deleted":
      return (
        isDocumentId(value.documentId) &&
        isWorkspaceRelativePath(value.previousPath) &&
        isRevision(value.revision) &&
        isWorkspaceGeneration(value.workspaceGeneration) &&
        hasExactKeys(value, [
          "type",
          "documentId",
          "previousPath",
          "revision",
          "workspaceGeneration",
        ])
      );
    case "settings-changed":
      return isSettingsSnapshot(value.settings) && hasExactKeys(value, ["type", "settings"]);
    case "sync-config-changed":
      return isSyncConfig(value.config) && hasExactKeys(value, ["type", "config"]);
    case "sync-status-changed":
      return isSyncStatus(value.status) && hasExactKeys(value, ["type", "status"]);
    default:
      return false;
  }
}

export function isWorkspace(value: unknown): value is Schemas["WorkspaceDto"] {
  if (!isRecord(value)) return false;
  return (
    isUuid(value.id) &&
    isWorkspaceGeneration(value.generation) &&
    typeof value.displayName === "string" &&
    (value.readiness === "ready" ||
      value.readiness === "initializing" ||
      value.readiness === "unavailable" ||
      value.readiness === "locked") &&
    isRevision(value.revision) &&
    hasExactKeys(value, ["id", "generation", "displayName", "readiness", "revision"])
  );
}

export function isDocumentEntry(value: unknown): value is Schemas["DocumentEntryDto"] {
  if (!isRecord(value)) return false;
  return (
    isDocumentId(value.id) &&
    (value.kind === "file" || value.kind === "directory") &&
    typeof value.modifiedAt === "string" &&
    isRfc3339Utc(value.modifiedAt) &&
    isDocumentName(value.name, value.kind) &&
    isWorkspaceRelativePath(value.parent) &&
    isWorkspaceRelativePath(value.path) &&
    isRevision(value.revision) &&
    isNonNegativeSafeInteger(value.sizeBytes) &&
    hasExactKeys(value, [
      "id",
      "kind",
      "modifiedAt",
      "name",
      "parent",
      "path",
      "revision",
      "sizeBytes",
    ])
  );
}

export function isSettingsSnapshot(value: unknown): value is Schemas["SettingsSnapshotDto"] {
  if (!isRecord(value)) return false;
  const seenKeys = new Set<string>();
  return (
    isRevision(value.revision) &&
    Array.isArray(value.values) &&
    value.values.every(
      (entry) =>
        isSettingEntry(entry) &&
        !seenKeys.has(entry.key) &&
        seenKeys.add(entry.key) !== undefined,
    ) &&
    hasExactKeys(value, ["revision", "values"])
  );
}

function isSettingEntry(value: unknown) {
  if (!isRecord(value)) return false;
  return (
    typeof value.key === "string" &&
    SETTING_KEYS.has(value.key) &&
    isSettingValue(value.value) &&
    isSettingValueForKey(value.key, value.value) &&
    hasExactKeys(value, ["key", "value"])
  );
}

function isSettingValueForKey(key: string, value: Schemas["SettingValueDto"]) {
  switch (key) {
    case "appearance.mode":
      return isStringValueIn(value, ["system", "light", "dark"]);
    case "appearance.lightTheme":
    case "appearance.darkTheme":
      return value.type === "string" && isThemeId(value.value);
    case "theme.customCss.light":
    case "theme.customCss.dark":
      return value.type === "string" && utf16Length(value.value) <= 50_000;
    case "language":
      return isStringValueIn(value, [
        "en",
        "zh-CN",
        "zh-TW",
        "ja",
        "ko",
        "fr",
        "de",
        "es",
        "pt-BR",
        "it",
        "ru",
      ]);
    case "editor.bodyFontSize":
      return isIntegerValueIn(value, [14, 15, 16, 17, 18, 20]);
    case "editor.contentWidth":
      return isStringValueIn(value, ["narrow", "default", "wide"]);
    case "editor.contentWidthPx":
      return (
        value.type === "nullable-integer" &&
        (value.value === null || integerBetween(value.value, 640, 1_280))
      );
    case "editor.fontFamily":
      return value.type === "font-family" && isFontFamily(value.value);
    case "editor.lineHeight":
      return value.type === "number" && [1.5, 1.65, 1.8].includes(value.value);
    case "editor.paragraphSpacingPx":
      return value.type === "integer" && integerBetween(value.value, 0, 32);
    case "editor.showWordCount":
    case "editor.wrapCodeBlocks":
    case "export.pdfPageBreakOnH1":
      return value.type === "boolean";
    case "editor.viewMode":
      return isStringValueIn(value, ["full", "daily", "focus", "immersive", "custom"]);
    case "files.ignoreRules":
      return value.type === "string" && new TextEncoder().encode(value.value).length <= 50_000;
    case "export.fontFamily":
      return (
        value.type === "nullable-string" &&
        (value.value === null || isFontName(value.value))
      );
    case "export.pdfAuthor":
    case "export.pdfFooter":
    case "export.pdfHeader":
      return value.type === "string" && utf16Length(value.value) <= 200;
    case "export.pdfHeightMm":
    case "export.pdfWidthMm":
      return value.type === "integer" && integerBetween(value.value, 50, 2_000);
    case "export.pdfMarginMm":
      return value.type === "integer" && integerBetween(value.value, 0, 60);
    case "export.pdfMarginPreset":
      return isStringValueIn(value, ["custom", "default", "narrow", "none", "normal", "wide"]);
    case "export.pdfPageSize":
      return isStringValueIn(value, ["a4", "custom", "default", "letter"]);
    default:
      return false;
  }
}

function isSettingValue(value: unknown): value is Schemas["SettingValueDto"] {
  if (!isRecord(value)) return false;
  if (!hasExactKeys(value, ["type", "value"])) return false;
  switch (value.type) {
    case "boolean":
      return typeof value.value === "boolean";
    case "integer":
      return Number.isSafeInteger(value.value);
    case "number":
      return typeof value.value === "number" && Number.isFinite(value.value);
    case "string":
      return typeof value.value === "string";
    case "nullable-integer":
      return value.value === null || Number.isSafeInteger(value.value);
    case "nullable-string":
      return value.value === null || typeof value.value === "string";
    case "font-family":
      return isFontFamily(value.value);
    default:
      return false;
  }
}

function isFontFamily(value: unknown) {
  if (!isRecord(value) || !hasExactKeys(value, ["source", "family"])) return false;
  return value.source === "theme"
    ? value.family === null
    : value.source === "system" && isFontName(value.family);
}

export function isSyncConfig(value: unknown): value is Schemas["SyncConfigViewDto"] {
  if (!isRecord(value)) return false;
  return (
    typeof value.configured === "boolean" &&
    typeof value.enabled === "boolean" &&
    typeof value.generateConflictDocument === "boolean" &&
    isSafeIntegerBetween(value.intervalSeconds, 30, 43_200) &&
    Array.isArray(value.issues) &&
    value.issues.every(isSyncIssue) &&
    (value.mode === "automatic" ||
      value.mode === "startup-exit" ||
      value.mode === "fully-manual") &&
    (value.provider === "s3" || value.provider === "webdav") &&
    (value.readiness === "disabled" ||
      value.readiness === "incomplete" ||
      value.readiness === "ready") &&
    typeof value.remoteRoot === "string" &&
    isRevision(value.revision) &&
    isS3Config(value.s3) &&
    isWebDavConfig(value.webdav) &&
    hasExactKeys(value, [
      "configured",
      "enabled",
      "generateConflictDocument",
      "intervalSeconds",
      "issues",
      "mode",
      "provider",
      "readiness",
      "remoteRoot",
      "revision",
      "s3",
      "webdav",
    ])
  );
}

function isSyncIssue(value: unknown) {
  if (!isRecord(value)) return false;
  return (
    (value.code === "required" ||
      value.code === "invalid-url" ||
      value.code === "unsafe-url-components" ||
      value.code === "out-of-range" ||
      value.code === "invalid-path") &&
    typeof value.field === "string" &&
    typeof value.message === "string" &&
    SAFE_SYNC_ISSUES.has(`${value.field}\u0000${value.code}\u0000${value.message}`) &&
    hasExactKeys(value, ["code", "field", "message"])
  );
}

const SAFE_SYNC_ISSUES = new Set([
  "remoteRoot\u0000invalid-path\u0000Remote root must be a safe relative path.",
  "webdav.serverUrl\u0000invalid-url\u0000Enter a valid HTTP or HTTPS URL.",
  "webdav.serverUrl\u0000unsafe-url-components\u0000Remove credentials, query parameters, and fragments from this URL.",
  "s3.endpointUrl\u0000invalid-url\u0000Enter a valid HTTP or HTTPS URL.",
  "s3.endpointUrl\u0000unsafe-url-components\u0000Remove credentials, query parameters, and fragments from this URL.",
  "s3.bucket\u0000required\u0000This field is required.",
  "s3.accessKeyId\u0000required\u0000This field is required.",
  "s3.secretAccessKey\u0000required\u0000This field is required.",
  "s3.requestTimeoutSeconds\u0000out-of-range\u0000Enter a value from 5 through 600.",
]);

function isS3Config(value: unknown): value is Schemas["S3ConfigViewDto"] {
  if (!isRecord(value)) return false;
  return (
    isCredentialState(value.accessKeyId) &&
    (value.addressingStyle === "auto" ||
      value.addressingStyle === "path" ||
      value.addressingStyle === "virtual-hosted") &&
    typeof value.bucket === "string" &&
    isSafeEndpoint(value.endpointUrl) &&
    typeof value.region === "string" &&
    isSafeIntegerBetween(value.requestTimeoutSeconds, 5, 600) &&
    isCredentialState(value.secretAccessKey) &&
    (value.tlsVerification === "verify" || value.tlsVerification === "skip") &&
    hasExactKeys(value, [
      "accessKeyId",
      "addressingStyle",
      "bucket",
      "endpointUrl",
      "region",
      "requestTimeoutSeconds",
      "secretAccessKey",
      "tlsVerification",
    ])
  );
}

function isWebDavConfig(value: unknown): value is Schemas["WebDavConfigViewDto"] {
  if (!isRecord(value)) return false;
  return (
    isCredentialState(value.password) &&
    isSafeEndpoint(value.serverUrl) &&
    typeof value.username === "string" &&
    hasExactKeys(value, ["password", "serverUrl", "username"])
  );
}

function isCredentialState(value: unknown) {
  return (
    isRecord(value) &&
    typeof value.present === "boolean" &&
    hasExactKeys(value, ["present"])
  );
}

function isSafeEndpoint(value: unknown) {
  if (
    !isRecord(value) ||
    typeof value.redacted !== "boolean" ||
    !hasExactKeys(value, ["redacted", "value"])
  ) {
    return false;
  }
  if (value.redacted) return value.value === null;
  if (value.value === null) return true;
  if (typeof value.value !== "string") return false;
  try {
    const endpoint = new URL(value.value);
    return (
      (endpoint.protocol === "http:" || endpoint.protocol === "https:") &&
      endpoint.hostname !== "" &&
      endpoint.username === "" &&
      endpoint.password === "" &&
      endpoint.search === "" &&
      endpoint.hash === ""
    );
  } catch {
    return false;
  }
}

export function isSyncStatus(value: unknown): value is Schemas["SyncStatusDto"] {
  if (!isRecord(value)) return false;
  return (
    (value.activeRunId === null || isUuid(value.activeRunId)) &&
    (value.completionState === "idle" ||
      value.completionState === "attempting" ||
      value.completionState === "failed" ||
      value.completionState === "succeeded") &&
    (value.configRevision === null || isRevision(value.configRevision)) &&
    (value.error === null || isSyncSafeError(value.error)) &&
    (value.lastAttemptAt === null || isRfc3339Utc(value.lastAttemptAt)) &&
    (value.lastSuccessfulSyncAt === null || isRfc3339Utc(value.lastSuccessfulSyncAt)) &&
    (value.lastTrigger === null ||
      value.lastTrigger === "app-launch" ||
      value.lastTrigger === "interval" ||
      value.lastTrigger === "manual" ||
      value.lastTrigger === "save" ||
      value.lastTrigger === "settings-exit") &&
    (value.provider === "s3" || value.provider === "webdav") &&
    (value.summary === null || isSyncSummary(value.summary)) &&
    isSyncStatusPhaseConsistent(value) &&
    hasExactKeys(value, [
      "activeRunId",
      "completionState",
      "configRevision",
      "error",
      "lastAttemptAt",
      "lastSuccessfulSyncAt",
      "lastTrigger",
      "provider",
      "summary",
    ])
  );
}

function isSyncStatusPhaseConsistent(value: Record<string, unknown>) {
  switch (value.completionState) {
    case "attempting":
      return value.activeRunId !== null && value.error === null;
    case "failed":
      return value.activeRunId === null && value.error !== null;
    case "idle":
    case "succeeded":
      return value.activeRunId === null && value.error === null;
    default:
      return false;
  }
}

function isSyncSafeError(value: unknown) {
  if (!isRecord(value)) return false;
  const allowed = [
    "category",
    "code",
    "httpStatus",
    "method",
    "operation",
    "provider",
    "providerErrorCode",
    "relativePath",
    "requestId",
    "runId",
  ];
  return (
    typeof value.code === "string" && SYNC_ERROR_CODES.has(value.code) &&
    typeof value.operation === "string" && SYNC_OPERATIONS.has(value.operation) &&
    (value.provider === "s3" || value.provider === "webdav") &&
    (value.category === undefined || (typeof value.category === "string" && SYNC_ERROR_CATEGORIES.has(value.category))) &&
    (value.httpStatus === undefined ||
      (Number.isInteger(value.httpStatus) &&
        typeof value.httpStatus === "number" &&
        value.httpStatus >= 100 &&
        value.httpStatus <= 599)) &&
    (value.method === undefined || (typeof value.method === "string" && SYNC_METHODS.has(value.method))) &&
    (value.providerErrorCode === undefined || (typeof value.providerErrorCode === "string" && SYNC_PROVIDER_ERROR_CODES.has(value.providerErrorCode))) &&
    (value.relativePath === undefined || isWorkspaceRelativePath(value.relativePath)) &&
    (value.requestId === undefined || isUuid(value.requestId)) &&
    (value.runId === undefined || isUuid(value.runId)) &&
    hasOnlyKeys(value, allowed)
  );
}

const SYNC_ERROR_CATEGORIES = new Set(["authentication", "authorization", "configuration", "conflict", "network", "provider", "storage", "transport"]);
const SYNC_ERROR_CODES = new Set(["authentication_failed", "cancelled", "configuration_invalid", "conflict", "connection_failed", "local_io", "permission_denied", "rate_limited", "remote_unavailable", "request_failed", "unknown"]);
const SYNC_OPERATIONS = new Set(["apply_config", "delete_object", "download_object", "list_remote", "read_local", "read_manifest", "sync_run", "test_connection", "upload_object", "write_local", "write_manifest"]);
const SYNC_METHODS = new Set(["DELETE", "GET", "HEAD", "POST", "PROPFIND", "PUT"]);
const SYNC_PROVIDER_ERROR_CODES = new Set(["AccessDenied", "Conflict", "Forbidden", "InvalidRequest", "Locked", "NoSuchBucket", "NoSuchKey", "NotFound", "PreconditionFailed", "RequestTimeout", "ServerError", "SlowDown", "TooManyRequests", "Unauthorized", "Unknown"]);

function isSyncSummary(value: unknown) {
  if (!isRecord(value)) return false;
  const keys = [
    "bytesDownloaded",
    "bytesUploaded",
    "conflictFiles",
    "downloadedFiles",
    "scannedFiles",
    "skippedFiles",
    "uploadedFiles",
  ];
  return keys.every((key) => isNonNegativeSafeInteger(value[key])) && hasExactKeys(value, keys);
}

function isSafeIntegerBetween(value: unknown, minimum: number, maximum: number) {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= minimum &&
    value <= maximum
  );
}

function integerBetween(value: number, minimum: number, maximum: number) {
  return Number.isSafeInteger(value) && value >= minimum && value <= maximum;
}

function isIntegerValueIn(
  value: Schemas["SettingValueDto"],
  allowed: readonly number[],
) {
  return value.type === "integer" && allowed.includes(value.value);
}

function isStringValueIn(
  value: Schemas["SettingValueDto"],
  allowed: readonly string[],
) {
  return value.type === "string" && allowed.includes(value.value);
}

function isThemeId(value: string) {
  return (
    value.length <= 64 &&
    !value.startsWith("qingyu-") &&
    /^[a-z0-9][a-z0-9-]*$/u.test(value)
  );
}

function isFontName(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value !== "" &&
    value.trim() === value &&
    utf16Length(value) <= 160 &&
    !/[\u0000-\u001f\u007f]/u.test(value)
  );
}

function utf16Length(value: string) {
  return [...value].reduce(
    (length, character) => length + (character.codePointAt(0)! > 0xffff ? 2 : 1),
    0,
  );
}

function isNonNegativeSafeInteger(value: unknown) {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}


function isUuid(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu.test(
      value,
    )
  );
}

function isDocumentId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length <= 8_192 &&
    /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/u.test(value)
  );
}

function isRevision(value: unknown): value is string {
  return typeof value === "string" && value !== "";
}

function isWorkspaceGeneration(value: unknown): value is string {
  return typeof value === "string" && value !== "";
}

function isWorkspaceRelativePath(value: unknown): value is string {
  if (typeof value !== "string") return false;
  if (value === "") return true;
  if (
    value.startsWith("/") ||
    value.startsWith("\\") ||
    value.includes("\\") ||
    /[\u0000-\u001f\u007f]/u.test(value) ||
    /^[A-Za-z]:/u.test(value)
  ) {
    return false;
  }
  return value.split("/").every((segment) => segment !== "" && segment !== "." && segment !== "..");
}

function isDocumentName(value: unknown, kind: unknown): value is string {
  if (
    typeof value !== "string" ||
    value === "" ||
    new TextEncoder().encode(value).length > 255 ||
    value === "." ||
    value === ".." ||
    value.endsWith(".") ||
    value.endsWith(" ") ||
    /[\u0000-\u001f\u007f/\\<>:"|?*]/u.test(value)
  ) {
    return false;
  }
  if (kind === "file" && !/\.(?:md|markdown)$/iu.test(value)) return false;
  const lower = value.toLocaleLowerCase("en-US");
  if (
    lower === ".qingyu" ||
    lower.startsWith(".qingyu-ui-update-") ||
    lower.startsWith(".qingyu-mcp-update-") ||
    lower.startsWith(".markra-sync-stage-")
  ) {
    return false;
  }
  const stem = value.split(".")[0]?.toLocaleUpperCase("en-US") ?? "";
  return !/^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/u.test(stem);
}

function hasExactKeys(value: Record<string, unknown>, keys: readonly string[]) {
  return Object.keys(value).length === keys.length && hasOnlyKeys(value, keys);
}

function hasOnlyKeys(value: Record<string, unknown>, keys: readonly string[]) {
  return Object.keys(value).every((key) => keys.includes(key));
}

const SETTING_KEYS = new Set([
  "appearance.mode",
  "appearance.lightTheme",
  "appearance.darkTheme",
  "theme.customCss.light",
  "theme.customCss.dark",
  "language",
  "editor.bodyFontSize",
  "editor.contentWidth",
  "editor.contentWidthPx",
  "editor.fontFamily",
  "editor.lineHeight",
  "editor.paragraphSpacingPx",
  "editor.showWordCount",
  "editor.wrapCodeBlocks",
  "editor.viewMode",
  "files.ignoreRules",
  "export.fontFamily",
  "export.pdfAuthor",
  "export.pdfFooter",
  "export.pdfHeader",
  "export.pdfHeightMm",
  "export.pdfWidthMm",
  "export.pdfMarginMm",
  "export.pdfMarginPreset",
  "export.pdfPageBreakOnH1",
  "export.pdfPageSize",
]);
