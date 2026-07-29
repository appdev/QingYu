import type { components } from "./generated/kernel-v1.ts";
import {
  KernelApiError,
  KERNEL_API_ERROR_MESSAGES,
  KernelProtocolError,
  KernelTransportError,
  type KernelApiErrorCode,
} from "./errors.ts";

export type FetchLike = (url: string, init?: RequestInit) => Promise<Response>;

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH";

export type HttpQuery = Readonly<
  Record<string, string | number | boolean | null | undefined>
>;

export interface HttpRequest {
  method: HttpMethod;
  path: string;
  query?: HttpQuery;
  body?: unknown;
  signal?: AbortSignal;
  authenticated?: boolean;
}

export interface HttpSuccessContract<Result> {
  status: number;
  validate?: (value: unknown) => value is Result;
}

export interface KernelHttpTransportOptions {
  baseUrl: string | URL;
  fetch: FetchLike;
  auth: KernelAuthentication;
}

export interface NativeBearerAuthentication {
  kind: "native-bearer";
  getCredential: () => string;
}

export type KernelAuthentication = NativeBearerAuthentication;

type ApiErrorEnvelope = components["schemas"]["ApiErrorEnvelope"];

export class KernelHttpTransport {
  readonly #baseUrl: URL;
  readonly #fetch: FetchLike;
  readonly #auth: KernelAuthentication;

  constructor(options: KernelHttpTransportOptions) {
    this.#baseUrl = parseKernelBaseUrl(options.baseUrl);
    this.#fetch = options.fetch;
    if (options.auth?.kind !== "native-bearer" || typeof options.auth.getCredential !== "function") {
      throw new KernelTransportError("unsupported-authentication");
    }
    this.#auth = options.auth;
  }

  async request<Result = unknown>(
    request: HttpRequest,
    success?: HttpSuccessContract<Result>,
  ): Promise<Result> {
    const url = this.#requestUrl(request.path, request.query);
    const headers = new Headers();
    if (request.authenticated !== false) {
      try {
        const credential = this.#auth.getCredential();
        if (
          typeof credential !== "string" ||
          credential === "" ||
          /[\u0000-\u001f\u007f]/u.test(credential)
        ) {
          throw new Error("invalid credential");
        }
        headers.set("authorization", `Bearer ${credential}`);
      } catch {
        throw new KernelTransportError("credential-unavailable");
      }
    }

    let body: string | undefined;
    if (request.body !== undefined) {
      headers.set("content-type", "application/json");
      try {
        body = JSON.stringify(request.body);
      } catch {
        throw new KernelTransportError("invalid-request");
      }
    }

    let response: Response;
    try {
      response = await this.#fetch(url.toString(), {
        method: request.method,
        headers,
        body,
        signal: request.signal,
        redirect: "error",
      });
    } catch (error: unknown) {
      const aborted =
        request.signal?.aborted === true ||
        (typeof error === "object" && error !== null && "name" in error && error.name === "AbortError");
      throw new KernelTransportError(aborted ? "aborted" : "network");
    }

    const requestId = responseRequestId(response);

    if (!response.ok) {
      throw await apiError(response, requestId);
    }
    if (success !== undefined && response.status !== success.status) {
      throw new KernelProtocolError("invalid-http-response", {
        status: response.status,
        requestId,
      });
    }
    if (response.status === 204) {
      if (success !== undefined && success.status !== 204) {
        throw new KernelProtocolError("invalid-http-response", {
          status: response.status,
          requestId,
        });
      }
      return undefined as Result;
    }

    try {
      const body: unknown = await response.json();
      if (success?.validate !== undefined && !success.validate(body)) {
        throw new KernelProtocolError("invalid-http-response", {
          status: response.status,
          requestId,
        });
      }
      return body as Result;
    } catch {
      throw new KernelProtocolError("invalid-http-response", {
        status: response.status,
        requestId,
      });
    }
  }

  #requestUrl(path: string, query: HttpQuery | undefined) {
    if (!path.startsWith("/api/v1/") || path.includes("?") || path.includes("#")) {
      throw new KernelTransportError("invalid-request");
    }
    const url = new URL(path, this.#baseUrl);
    for (const [name, value] of Object.entries(query ?? {})) {
      if (value !== undefined && value !== null) {
        url.searchParams.set(name, String(value));
      }
    }
    return url;
  }
}

export function parseKernelBaseUrl(value: string | URL) {
  const raw = typeof value === "string" ? value : value.href;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new KernelTransportError("invalid-base-url");
  }
  if (
    (url.protocol !== "http:" && url.protocol !== "https:") ||
    url.username !== "" ||
    url.password !== "" ||
    url.search !== "" ||
    url.hash !== "" ||
    url.pathname !== "/" ||
    !isExplicitLoopback(raw, url)
  ) {
    throw new KernelTransportError("invalid-base-url");
  }
  return url;
}

function isExplicitLoopback(raw: string, parsed: URL) {
  const authority = /^https?:\/\/([^/?#]+)\/?$/iu.exec(raw)?.[1];
  if (authority === undefined || authority.includes("@")) return false;

  let host: string;
  if (authority.startsWith("[")) {
    const closingBracket = authority.indexOf("]");
    if (closingBracket < 0 || !/^(?::\d+)?$/u.test(authority.slice(closingBracket + 1))) {
      return false;
    }
    host = authority.slice(0, closingBracket + 1);
  } else {
    const match = /^([^:]+)(?::\d+)?$/u.exec(authority);
    if (match === null) return false;
    host = match[1]!;
  }

  if (host.toLowerCase() === "localhost") {
    return parsed.hostname === "localhost";
  }
  if (host.toLowerCase() === "[::1]") {
    return parsed.hostname === "[::1]";
  }
  const octets = host.split(".");
  return (
    octets.length === 4 &&
    octets[0] === "127" &&
    octets.every(
      (octet) =>
        /^(?:0|[1-9]\d{0,2})$/u.test(octet) && Number(octet) <= 255,
    ) &&
    parsed.hostname === host
  );
}

function responseRequestId(response: Response) {
  const requestId = response.headers.get("x-request-id");
  if (!isUuid(requestId)) {
    throw new KernelProtocolError("invalid-http-response", {
      status: response.status,
    });
  }
  return requestId;
}

async function apiError(response: Response, headerRequestId: string) {
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    return new KernelProtocolError("invalid-http-response", {
      status: response.status,
      requestId: headerRequestId,
    });
  }
  if (!isApiErrorEnvelope(body, response.status) || headerRequestId !== body.requestId) {
    return new KernelProtocolError("invalid-http-response", {
      status: response.status,
      requestId: headerRequestId,
    });
  }
  return new KernelApiError({
    code: body.code,
    status: response.status,
    requestId: body.requestId,
    details: body.details,
  });
}

function isApiErrorEnvelope(value: unknown, status: number): value is ApiErrorEnvelope {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    isApiErrorCode(candidate.code) &&
    candidate.message === KERNEL_API_ERROR_MESSAGES[candidate.code as KernelApiErrorCode] &&
    ERROR_STATUS[candidate.code as KernelApiErrorCode] === status &&
    isUuid(candidate.requestId) &&
    isErrorDetails(candidate.details, candidate.code as KernelApiErrorCode) &&
    hasOnlyKeys(candidate, ["code", "message", "requestId", "details"])
  );
}

const API_ERROR_CODES: ReadonlySet<KernelApiErrorCode> = new Set([
  "invalid_request",
  "invalid_workspace_path",
  "invalid_document_name",
  "unauthorized",
  "host_not_allowed",
  "origin_not_allowed",
  "kernel_not_ready",
  "workspace_unavailable",
  "workspace_locked",
  "document_not_found",
  "document_already_exists",
  "document_too_large",
  "document_invalid_encoding",
  "revision_conflict",
  "settings_revision_conflict",
  "sync_config_revision_conflict",
  "invalid_settings_field",
  "settings_unavailable",
  "sync_config_absent",
  "sync_config_invalid",
  "sync_not_ready",
  "sync_run_unavailable",
  "internal_error",
]);

const ERROR_STATUS: Record<KernelApiErrorCode, number> = {
  invalid_request: 400, invalid_workspace_path: 400, invalid_document_name: 400,
  unauthorized: 401, host_not_allowed: 403, origin_not_allowed: 403,
  document_not_found: 404, sync_config_absent: 404,
  document_already_exists: 409, revision_conflict: 409, settings_revision_conflict: 409,
  sync_config_revision_conflict: 409, document_too_large: 413,
  document_invalid_encoding: 422, invalid_settings_field: 422, sync_config_invalid: 422,
  workspace_locked: 423, kernel_not_ready: 503, workspace_unavailable: 503,
  settings_unavailable: 503, sync_not_ready: 503, sync_run_unavailable: 503,
  internal_error: 500,
};

const STARTUP_STATES = new Set([
  "starting",
  "needs-owner",
  "needs-workspace-initialization",
  "needs-cloud-binding",
  "ready",
  "recoverable-error",
  "fatal-error",
]);

const VALIDATION_CODES = new Set([
  "required",
  "invalid-format",
  "out-of-range",
  "conflict",
  "unsafe-value",
]);

const VALIDATION_MESSAGES = new Set([
  "This field is required.",
  "This field has an invalid format.",
  "This field is outside the supported range.",
  "This field conflicts with another value.",
  "This field contains an unsafe value.",
]);

const VALIDATION_FIELDS = new Set([
  "request", "workspaceGeneration", "parent", "name", "kind", "contents",
  "expectedRevision", "targetParent", "deletionPolicy", "cursor", "limit", "query",
  "snapshotId", "values", "changes", "provider", "mode", "remoteRoot",
  "intervalSeconds", "webdav", "s3", "endpointUrl", "username", "password",
  "accessKeyId", "secretAccessKey", "bucket", "region", "addressingStyle",
  "tlsVerification", "expectedConfigRevision",
]);

function isApiErrorCode(value: unknown): value is KernelApiErrorCode {
  return typeof value === "string" && API_ERROR_CODES.has(value as KernelApiErrorCode);
}

function isErrorDetails(value: unknown, code: KernelApiErrorCode) {
  if (value === undefined) return true;
  if (typeof value !== "object" || value === null) return false;
  const details = value as Record<string, unknown>;
  switch (details.type) {
    case "revision-conflict":
      return (
        ["revision_conflict", "settings_revision_conflict", "sync_config_revision_conflict"].includes(code) &&
        (details.currentRevision === undefined || (typeof details.currentRevision === "string" && details.currentRevision !== "")) &&
        hasOnlyKeys(details, ["type", "currentRevision"])
      );
    case "startup":
      return (
        ["kernel_not_ready", "workspace_unavailable", "workspace_locked", "settings_unavailable", "sync_not_ready", "sync_run_unavailable"].includes(code) &&
        typeof details.state === "string" &&
        STARTUP_STATES.has(details.state) &&
        hasOnlyKeys(details, ["type", "state"])
      );
    case "validation":
      return (
        ["invalid_request", "invalid_workspace_path", "invalid_document_name", "document_too_large", "document_invalid_encoding", "invalid_settings_field", "sync_config_invalid"].includes(code) &&
        Array.isArray(details.issues) &&
        details.issues.length > 0 &&
        details.issues.every(isValidationIssue) &&
        hasOnlyKeys(details, ["type", "issues"])
      );
    default:
      return false;
  }
}

function isValidationIssue(value: unknown) {
  if (typeof value !== "object" || value === null) return false;
  const issue = value as Record<string, unknown>;
  return (
    typeof issue.code === "string" &&
    VALIDATION_CODES.has(issue.code) &&
    typeof issue.field === "string" &&
    VALIDATION_FIELDS.has(issue.field) &&
    typeof issue.message === "string" &&
    VALIDATION_MESSAGES.has(issue.message) &&
    hasOnlyKeys(issue, ["code", "field", "message"])
  );
}

function hasOnlyKeys(value: Record<string, unknown>, allowed: readonly string[]) {
  return Object.keys(value).every((key) => allowed.includes(key));
}

function isUuid(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu.test(
      value,
    )
  );
}
