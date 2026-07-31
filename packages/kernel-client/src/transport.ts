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

export interface HttpBinarySuccessContract {
  status: number;
  mediaTypes: readonly string[];
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

export interface BrowserSessionAuthentication {
  kind: "browser-session";
  browserOrigin: string | URL;
  getCsrfToken: () => string | null | undefined;
}

export type KernelAuthentication =
  | NativeBearerAuthentication
  | BrowserSessionAuthentication;

type ApiErrorEnvelope = components["schemas"]["ApiErrorEnvelope"];

export class KernelHttpTransport {
  readonly #baseUrl: URL;
  readonly #fetch: FetchLike;
  readonly #auth: KernelAuthentication;

  constructor(options: KernelHttpTransportOptions) {
    const authentication = snapshotKernelAuthentication(options.auth);
    this.#baseUrl = parseKernelBaseUrl(options.baseUrl, authentication);
    this.#fetch = options.fetch;
    this.#auth = authentication;
  }

  async request<Result = unknown>(
    request: HttpRequest,
    success?: HttpSuccessContract<Result>,
  ): Promise<Result> {
    const { response, requestId } = await this.#send(request);
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

  async requestBinary(
    request: HttpRequest,
    success: HttpBinarySuccessContract,
  ): Promise<Response> {
    const { response, requestId } = await this.#send(request);
    const contentType = response.headers.get("content-type");
    const contentLength = response.headers.get("content-length");
    const length = contentLength !== null && /^(?:0|[1-9]\d*)$/u.test(contentLength)
      ? Number(contentLength)
      : Number.NaN;
    if (
      response.status !== success.status ||
      contentType === null ||
      !success.mediaTypes.includes(contentType) ||
      !Number.isSafeInteger(length) ||
      length < 0 ||
      response.headers.get("x-content-type-options") !== "nosniff" ||
      (length > 0 && response.body === null)
    ) {
      throw new KernelProtocolError("invalid-http-response", {
        status: response.status,
        requestId,
      });
    }
    return response;
  }

  async #send(request: HttpRequest): Promise<{ response: Response; requestId: string }> {
    if ("credentials" in request) {
      throw new KernelTransportError("invalid-request");
    }
    const url = this.#requestUrl(request.path, request.query);
    const headers = new Headers();
    if (this.#auth.kind === "native-bearer" && request.authenticated !== false) {
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
    } else if (this.#auth.kind === "browser-session" && requiresCsrf(request)) {
      let csrfToken: string | null | undefined;
      try {
        csrfToken = this.#auth.getCsrfToken();
      } catch {
        throw new KernelTransportError("csrf-unavailable");
      }
      if (
        typeof csrfToken !== "string" ||
        !/^[A-Za-z0-9_-]+$/u.test(csrfToken)
      ) {
        throw new KernelTransportError("csrf-unavailable");
      }
      headers.set("x-csrf-token", csrfToken);
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
      const init: RequestInit = {
        method: request.method,
        headers,
        body,
        signal: request.signal,
        redirect: "error",
      };
      if (this.#auth.kind === "browser-session") {
        init.credentials = "same-origin";
      }
      response = await this.#fetch(url.toString(), init);
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
    return { response, requestId };
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

export function parseKernelBaseUrl(
  value: string | URL,
  authentication?: KernelAuthentication,
) {
  const raw = typeof value === "string" ? value : value.href;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new KernelTransportError("invalid-base-url");
  }
  const invalidCommon =
    (url.protocol !== "http:" && url.protocol !== "https:") ||
    url.username !== "" ||
    url.password !== "" ||
    url.search !== "" ||
    url.hash !== "" ||
    url.pathname !== "/";
  const validEndpoint = authentication?.kind === "browser-session"
    ? isExactBrowserOrigin(url, authentication.browserOrigin)
    : isExplicitLoopback(raw, url);
  if (invalidCommon || !validEndpoint) {
    throw new KernelTransportError("invalid-base-url");
  }
  return url;
}

function isKernelAuthentication(value: unknown): value is KernelAuthentication {
  if (typeof value !== "object" || value === null || !("kind" in value)) {
    return false;
  }
  if (value.kind === "native-bearer") {
    return "getCredential" in value && typeof value.getCredential === "function";
  }
  return value.kind === "browser-session" &&
    "getCsrfToken" in value && typeof value.getCsrfToken === "function" &&
    "browserOrigin" in value &&
    (typeof value.browserOrigin === "string" || value.browserOrigin instanceof URL);
}

export function snapshotKernelAuthentication(value: unknown): KernelAuthentication {
  if (!isKernelAuthentication(value)) {
    throw new KernelTransportError("unsupported-authentication");
  }
  if (value.kind === "native-bearer") {
    return {
      kind: "native-bearer",
      getCredential: value.getCredential.bind(value),
    };
  }
  return {
    kind: "browser-session",
    browserOrigin: typeof value.browserOrigin === "string"
      ? value.browserOrigin
      : value.browserOrigin.href,
    getCsrfToken: value.getCsrfToken.bind(value),
  };
}

function isExactBrowserOrigin(baseUrl: URL, browserOriginValue: string | URL) {
  let browserOrigin: URL;
  try {
    browserOrigin = new URL(browserOriginValue);
  } catch {
    return false;
  }
  return (
    (baseUrl.protocol === "http:" || baseUrl.protocol === "https:") &&
    baseUrl.protocol === browserOrigin.protocol &&
    browserOrigin.username === "" &&
    browserOrigin.password === "" &&
    browserOrigin.pathname === "/" &&
    browserOrigin.search === "" &&
    browserOrigin.hash === "" &&
    baseUrl.origin === browserOrigin.origin
  );
}

function requiresCsrf(request: HttpRequest) {
  if (request.method === "GET") return false;
  return !(
    request.method === "POST" &&
    (request.path === "/api/v1/auth/initialize" || request.path === "/api/v1/auth/session")
  );
}

function isExplicitLoopback(raw: string, parsed: URL) {
  const match = /^http:\/\/127\.0\.0\.1:([1-9]\d{0,4})\/?$/u.exec(raw);
  if (match === null) return false;
  const port = match[1]!;
  return (
    Number(port) <= 65_535 &&
    parsed.protocol === "http:" &&
    parsed.hostname === "127.0.0.1" &&
    parsed.port === port
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
  if (
    !isApiErrorEnvelope(body, response.status) ||
    headerRequestId !== body.requestId ||
    !hasMatchingRetryAfter(response, body)
  ) {
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
  "initialization_required",
  "already_initialized",
  "invalid_credentials",
  "csrf_rejected",
  "authentication_rate_limited",
  "authentication_unavailable",
  "host_not_allowed",
  "origin_not_allowed",
  "kernel_not_ready",
  "workspace_unavailable",
  "workspace_locked",
  "document_not_found",
  "resource_not_found",
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
  unauthorized: 401, invalid_credentials: 401,
  host_not_allowed: 403, origin_not_allowed: 403, csrf_rejected: 403,
  document_not_found: 404, resource_not_found: 404, sync_config_absent: 404,
  document_already_exists: 409, initialization_required: 409, already_initialized: 409,
  revision_conflict: 409, settings_revision_conflict: 409,
  sync_config_revision_conflict: 409, document_too_large: 413,
  document_invalid_encoding: 422, invalid_settings_field: 422, sync_config_invalid: 422,
  workspace_locked: 423, authentication_rate_limited: 429,
  kernel_not_ready: 503, authentication_unavailable: 503, workspace_unavailable: 503,
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
  if (value === undefined) return code !== "authentication_rate_limited";
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
    case "rate-limit":
      return (
        code === "authentication_rate_limited" &&
        Number.isSafeInteger(details.retryAfterSeconds) &&
        typeof details.retryAfterSeconds === "number" &&
        details.retryAfterSeconds >= 1 &&
        hasOnlyKeys(details, ["type", "retryAfterSeconds"])
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

function hasMatchingRetryAfter(response: Response, body: ApiErrorEnvelope) {
  if (body.code !== "authentication_rate_limited") return true;
  if (body.details?.type !== "rate-limit") return false;
  const retryAfter = parsePositiveSafeIntegerHeader(response.headers.get("retry-after"));
  return retryAfter !== undefined && retryAfter === body.details.retryAfterSeconds;
}

function parsePositiveSafeIntegerHeader(value: string | null) {
  if (value === null || !/^[1-9]\d*$/u.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : undefined;
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
