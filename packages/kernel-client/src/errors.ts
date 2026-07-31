import type { components } from "./generated/kernel-v1.ts";

export type KernelApiErrorCode = components["schemas"]["ErrorCode"];
export type KernelApiErrorDetails = components["schemas"]["ErrorDetails"];

export const KERNEL_API_ERROR_MESSAGES: Record<KernelApiErrorCode, string> = {
  invalid_request: "The request is invalid.", invalid_workspace_path: "The workspace path is invalid.", invalid_document_name: "The document name is invalid.",
  unauthorized: "Authentication is required.", initialization_required: "Server initialization is required.", already_initialized: "Server initialization is already complete.",
  invalid_credentials: "The credentials are invalid.", csrf_rejected: "The CSRF proof is invalid.", authentication_rate_limited: "Authentication is temporarily limited.",
  authentication_unavailable: "Authentication is unavailable.", host_not_allowed: "The request host is not allowed.", origin_not_allowed: "The request origin is not allowed.",
  kernel_not_ready: "The Kernel is not ready.", workspace_unavailable: "The workspace is unavailable.", workspace_locked: "The workspace is locked.",
  document_not_found: "The document was not found.", resource_not_found: "The resource was not found.", document_already_exists: "The document already exists.", document_too_large: "The document exceeds the supported size.", resource_too_large: "The resource exceeds the supported size.",
  document_invalid_encoding: "The document encoding is invalid.", revision_conflict: "The document changed since it was loaded.", settings_revision_conflict: "The settings changed since they were loaded.",
  sync_config_revision_conflict: "The sync configuration changed since it was loaded.", invalid_settings_field: "A settings field is invalid.", settings_unavailable: "Settings are unavailable.",
  sync_config_absent: "Sync is not configured.", sync_config_invalid: "The sync configuration is invalid.", sync_not_ready: "Sync is not ready.",
  sync_run_unavailable: "A sync run cannot be started now.", internal_error: "An unexpected error occurred.",
};

export class KernelApiError extends Error {
  readonly code: KernelApiErrorCode;
  readonly status: number;
  readonly requestId: string;
  readonly details: KernelApiErrorDetails | undefined;

  constructor(options: {
    code: KernelApiErrorCode;
    status: number;
    requestId: string;
    details?: KernelApiErrorDetails;
  }) {
    super(KERNEL_API_ERROR_MESSAGES[options.code]);
    this.name = "KernelApiError";
    this.code = options.code;
    this.status = options.status;
    this.requestId = options.requestId;
    this.details = options.details;
  }
}

export type KernelTransportErrorKind =
  | "aborted"
  | "credential-unavailable"
  | "csrf-unavailable"
  | "invalid-base-url"
  | "invalid-request"
  | "network"
  | "unsupported-authentication";

const TRANSPORT_ERROR_MESSAGES: Record<KernelTransportErrorKind, string> = {
  aborted: "The Kernel request was cancelled.",
  "credential-unavailable": "The Kernel credential is unavailable.",
  "csrf-unavailable": "The Kernel CSRF proof is unavailable.",
  "invalid-base-url": "The Kernel base URL is invalid.",
  "invalid-request": "The Kernel request is invalid.",
  network: "The Kernel request could not be completed.",
  "unsupported-authentication": "The Kernel authentication mode is unsupported.",
};

export class KernelTransportError extends Error {
  readonly kind: KernelTransportErrorKind;
  readonly status: number | undefined;
  readonly requestId: string | undefined;

  constructor(
    kind: KernelTransportErrorKind,
    options: { status?: number; requestId?: string } = {},
  ) {
    super(TRANSPORT_ERROR_MESSAGES[kind]);
    this.name = "KernelTransportError";
    this.kind = kind;
    this.status = options.status;
    this.requestId = options.requestId;
  }
}

export type KernelProtocolErrorKind =
  | "invalid-http-response"
  | "invalid-websocket-frame"
  | "unsupported-websocket-version";

const PROTOCOL_ERROR_MESSAGES: Record<KernelProtocolErrorKind, string> = {
  "invalid-http-response": "The Kernel returned an invalid HTTP response.",
  "invalid-websocket-frame": "The Kernel sent an invalid event frame.",
  "unsupported-websocket-version": "The Kernel event protocol version is unsupported.",
};

export class KernelProtocolError extends Error {
  readonly kind: KernelProtocolErrorKind;
  readonly status: number | undefined;
  readonly requestId: string | undefined;

  constructor(
    kind: KernelProtocolErrorKind,
    options: { status?: number; requestId?: string } = {},
  ) {
    super(PROTOCOL_ERROR_MESSAGES[kind]);
    this.name = "KernelProtocolError";
    this.kind = kind;
    this.status = options.status;
    this.requestId = options.requestId;
  }
}

export type KernelEventErrorKind = "connection" | "server-error";

const EVENT_ERROR_MESSAGES: Record<KernelEventErrorKind, string> = {
  connection: "The Kernel event connection failed.",
  "server-error": "The Kernel rejected the event connection.",
};

export class KernelEventError extends Error {
  readonly kind: KernelEventErrorKind;
  readonly frameCode: string | undefined;

  constructor(kind: KernelEventErrorKind, options: { frameCode?: string } = {}) {
    super(EVENT_ERROR_MESSAGES[kind]);
    this.name = "KernelEventError";
    this.kind = kind;
    this.frameCode = options.frameCode;
  }
}
