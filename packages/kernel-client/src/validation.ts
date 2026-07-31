import type { components } from "./generated/kernel-v1.ts";
import { isRfc3339Utc } from "./datetime.ts";
import {
  isDocumentEntry,
  isSettingsSnapshot,
  isSyncConfig,
  isSyncRunStatus,
  isSyncStatus,
  isWorkspace,
} from "./events.ts";

type Schemas = components["schemas"];

export function isServerAuthenticationStatus(
  value: unknown,
): value is Schemas["ServerAuthenticationStatusDto"] {
  return isRecord(value) &&
    (value.initialization === "required" ||
      value.initialization === "initialized" ||
      value.initialization === "unavailable") &&
    exact(value, ["initialization"]);
}

export function isServerSession(value: unknown): value is Schemas["ServerSessionDto"] {
  return isRecord(value) && value.state === "authenticated" && exact(value, ["state"]);
}

export function isLiveHealth(value: unknown): value is Schemas["LiveHealthResponse"] {
  return isRecord(value) && value.apiVersion === "v1" && value.status === "live" && exact(value, ["apiVersion", "status"]);
}

export function isReadyHealth(value: unknown): value is Schemas["ReadyHealthResponse"] {
  return isRecord(value) && value.apiVersion === "v1" && isUuid(value.instanceId) && value.status === "ready" && exact(value, ["apiVersion", "instanceId", "status"]);
}

export function isVersion(value: unknown): value is Schemas["SystemVersionResponse"] {
  return isRecord(value) && value.apiVersion === "v1" && isUuid(value.instanceId) && typeof value.kernelVersion === "string" && exact(value, ["apiVersion", "instanceId", "kernelVersion"]);
}

export function isRuntime(value: unknown): value is Schemas["RuntimeStateDto"] {
  if (!isRecord(value) || !isRecord(value.capabilities)) return false;
  const capabilities = value.capabilities;
  const capabilityKeys = ["documents", "history", "portableSettings", "resources", "s3", "search", "settings", "sync", "webdav"];
  return capabilityKeys.every((key) => typeof capabilities[key] === "boolean") && exact(capabilities, capabilityKeys) && isUuid(value.instanceId) && ["desktop", "server", "mobile"].includes(String(value.profile)) && ["starting", "needs-owner", "needs-workspace-initialization", "needs-cloud-binding", "ready", "recoverable-error", "fatal-error"].includes(String(value.startupState)) && exact(value, ["capabilities", "instanceId", "profile", "startupState"]);
}

export {
  isWorkspace,
  isDocumentEntry,
  isSettingsSnapshot,
  isSyncConfig,
  isSyncRunStatus,
  isSyncStatus,
};

export function isDocumentPage(value: unknown): value is Schemas["DocumentPageDto"] {
  return page(value, isDocumentEntry);
}

export function isInventoryPage(value: unknown): value is Schemas["WorkspaceInventoryPageDto"] {
  return page(value, (item) => {
    if (!isRecord(item)) return false;
    if (item.entryType === "document") {
      return isDocumentEntry(item.document) && exact(item, ["document", "entryType"]);
    }
    return item.entryType === "resource" && isResourceEntry(item.resource) && exact(item, ["entryType", "resource"]);
  });
}

export function isSearchPage(value: unknown): value is Schemas["SearchPageDto"] {
  return page(value, (item) => isRecord(item) && positive(item.column) && isDocumentEntry(item.document) && positive(item.line) && typeof item.preview === "string" && exact(item, ["column", "document", "line", "preview"]));
}

export function isCreatedDocument(value: unknown): value is Schemas["CreatedDocumentDto"] {
  if (!isDocumentBase(value)) return false;
  return value.kind === "file"
    ? isContents(value.contents) && exact(value, [...DOCUMENT_KEYS, "contents"])
    : value.kind === "directory" && exact(value, DOCUMENT_KEYS);
}

export function isDocumentContent(value: unknown): value is Schemas["DocumentContentDto"] {
  return isDocumentBase(value) && value.kind === "file" && isContents(value.contents) && exact(value, [...DOCUMENT_KEYS, "contents"]);
}

export function isHistoryPage(value: unknown): value is Schemas["DocumentHistoryPageDto"] {
  return page(value, (item) => isRecord(item) && isRfc3339Utc(item.createdAt) && isDocumentId(item.documentId) && isRevision(item.revision) && nonNegative(item.sizeBytes) && isUuid(item.snapshotId) && exact(item, ["createdAt", "documentId", "revision", "sizeBytes", "snapshotId"]));
}

export function isHistorySnapshot(value: unknown): value is Schemas["DocumentHistorySnapshotDto"] {
  return isRecord(value) &&
    isContents(value.contents) &&
    isRfc3339Utc(value.createdAt) &&
    isDocumentId(value.documentId) &&
    isRevision(value.revision) &&
    nonNegative(value.sizeBytes) &&
    isUuid(value.snapshotId) &&
    exact(value, ["contents", "createdAt", "documentId", "revision", "sizeBytes", "snapshotId"]);
}

export function isSyncConnection(value: unknown): value is Schemas["SyncConnectionTestDto"] {
  return isRecord(value) && typeof value.checkedTarget === "string" && isRevision(value.configRevision) && (value.provider === "s3" || value.provider === "webdav") && exact(value, ["checkedTarget", "configRevision", "provider"]);
}

export function isSyncRun(value: unknown): value is Schemas["SyncRunAcceptedDto"] {
  return isRecord(value) && isRfc3339Utc(value.acceptedAt) && isRevision(value.configRevision) && isUuid(value.runId) && exact(value, ["acceptedAt", "configRevision", "runId"]);
}

const DOCUMENT_KEYS = ["id", "kind", "modifiedAt", "name", "parent", "path", "revision", "sizeBytes"];

function isDocumentBase(value: unknown): value is Record<string, unknown> {
  if (!isRecord(value)) return false;
  const entry: Record<string, unknown> = {};
  for (const key of DOCUMENT_KEYS) entry[key] = value[key];
  return isDocumentEntry(entry);
}

function page(value: unknown, item: (candidate: unknown) => boolean) {
  return isRecord(value) && Array.isArray(value.items) && value.items.every(item) && (value.nextCursor === null || isCursor(value.nextCursor)) && exact(value, ["items", "nextCursor"]);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function exact(value: Record<string, unknown>, keys: readonly string[]) {
  return Object.keys(value).length === keys.length && Object.keys(value).every((key) => keys.includes(key));
}

function isUuid(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu.test(value);
}

function isDocumentId(value: unknown): value is string {
  return typeof value === "string" && value.length <= 8_192 && /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/u.test(value);
}

export function isResourceEntry(value: unknown): value is components["schemas"]["ResourceEntryDto"] {
  if (!isRecord(value)) return false;
  const expectedPath = isResourceName(value.name) && isWorkspaceRelativePath(value.parent)
    ? `${value.parent === "" ? "" : `${value.parent}/`}${value.name}`
    : null;
  const mediaMatchesKind = typeof value.mediaType === "string" && (value.kind === "image"
    ? value.previewable === true && RESOURCE_IMAGE_MEDIA_TYPES.has(value.mediaType) && imageExtensionMatchesMediaType(value.name, value.mediaType)
    : value.kind === "attachment" && value.previewable === false && value.mediaType === "application/octet-stream");
  return isResourceId(value.id) &&
    mediaMatchesKind &&
    isRfc3339Utc(value.modifiedAt) &&
    expectedPath !== null &&
    value.path === expectedPath &&
    isRevision(value.revision) &&
    nonNegative(value.sizeBytes) &&
    exact(value, ["id", "kind", "mediaType", "modifiedAt", "name", "parent", "path", "previewable", "revision", "sizeBytes"]);
}

const RESOURCE_IMAGE_MEDIA_TYPES = new Set([
  "image/avif", "image/bmp", "image/gif", "image/jpeg", "image/png", "image/svg+xml", "image/webp",
]);

function imageExtensionMatchesMediaType(name: unknown, mediaType: string) {
  if (typeof name !== "string") return false;
  const lower = name.toLocaleLowerCase("en-US");
  if (mediaType === "image/jpeg") return lower.endsWith(".jpg") || lower.endsWith(".jpeg");
  if (mediaType === "image/png") return lower.endsWith(".png");
  if (mediaType === "image/gif") return lower.endsWith(".gif");
  if (mediaType === "image/webp") return lower.endsWith(".webp");
  if (mediaType === "image/avif") return lower.endsWith(".avif");
  if (mediaType === "image/bmp") return lower.endsWith(".bmp");
  return mediaType === "image/svg+xml" && lower.endsWith(".svg");
}

export function isResourceBatchResponse(
  value: unknown,
): value is components["schemas"]["CreateWorkspaceResourceBatchResponse"] {
  return isRecord(value) &&
    isUuid(value.batchId) &&
    Array.isArray(value.resources) &&
    value.resources.length > 0 &&
    value.resources.every(isResourceEntry) &&
    exact(value, ["batchId", "resources"]);
}

function isResourceName(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    value === "" ||
    new TextEncoder().encode(value).length > 255 ||
    value === "." ||
    value === ".." ||
    value.endsWith(".") ||
    value.endsWith(" ") ||
    /[\u0000-\u001f\u007f-\u009f/\\<>:"|?*]/u.test(value)
  ) {
    return false;
  }
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

function isWorkspaceRelativePath(value: unknown): value is string {
  if (typeof value !== "string") return false;
  if (value === "") return true;
  if (
    value.startsWith("/") ||
    value.startsWith("\\") ||
    value.includes("\\") ||
    /[\u0000-\u001f\u007f-\u009f]/u.test(value) ||
    /^[A-Za-z]:/u.test(value)
  ) {
    return false;
  }
  return value.split("/").every((segment) => segment !== "" && segment !== "." && segment !== "..");
}

function isResourceId(value: unknown): value is string {
  return typeof value === "string" && value.length <= 8_192 && /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/u.test(value);
}

function isCursor(value: unknown): value is string {
  return typeof value === "string" && value.length <= 2_048 && /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/u.test(value);
}

function isRevision(value: unknown): value is string {
  return typeof value === "string" && value !== "";
}

function isContents(value: unknown): value is string {
  return typeof value === "string" && new TextEncoder().encode(value).length <= 16 * 1024 * 1024;
}

function positive(value: unknown) {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function nonNegative(value: unknown) {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}
