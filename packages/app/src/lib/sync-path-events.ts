import { normalizeComparablePath } from "./path-move";

export const syncPathGuardRequestEvent = "qingyu://sync-path-guard-request";
export const syncPathGuardReleaseEvent = "qingyu://sync-path-guard-release";

export type SyncPathGuardRequest = {
  requestId: string;
  jobId: string;
  notesRoot: string;
  relativePaths: string[];
};

export type SyncPathGuardRelease = {
  requestId: string;
  notesRoot: string;
  relativePaths: string[];
};

export type SyncPathMutation = {
  destinationPath?: string | null;
  sourcePath?: string | null;
};

const canonicalUuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function canonicalUuid(value: unknown): value is string {
  return typeof value === "string" && canonicalUuidPattern.test(value);
}

function canonicalNotesRoot(value: unknown): value is string {
  if (typeof value !== "string" || value !== value.trim()) return false;
  if (value.replace(/\\/gu, "/").split("/").some((component) => component === "." || component === "..")) {
    return false;
  }
  const normalized = normalizeComparablePath(value);
  if (!normalized || normalized !== normalizeComparablePath(normalized)) return false;
  return normalized.startsWith("/") || /^[a-z]:\//u.test(normalized) || normalized.startsWith("//");
}

function repositoryRelativePath(value: unknown): value is string {
  if (typeof value !== "string" || !value || value.startsWith("/") || value.includes("\\")) {
    return false;
  }
  if ([...value].some((character) => character.charCodeAt(0) < 32)) return false;

  return value.split("/").every((component) => (
    component.length > 0 &&
    component !== "." &&
    component !== ".." &&
    !component.includes(":") &&
    !component.endsWith(".") &&
    !component.endsWith(" ")
  ));
}

function relativePaths(value: unknown): string[] | null {
  if (!Array.isArray(value) || value.length === 0 || !value.every(repositoryRelativePath)) return null;
  const paths = value as string[];
  if (new Set(paths).size !== paths.length) return null;
  return [...paths];
}

export function parseSyncPathGuardRequest(value: unknown): SyncPathGuardRequest | null {
  const input = record(value);
  if (!input || !canonicalUuid(input.requestId) || !canonicalUuid(input.jobId) || !canonicalNotesRoot(input.notesRoot)) {
    return null;
  }
  const paths = relativePaths(input.relativePaths);
  if (!paths) return null;

  return {
    jobId: input.jobId,
    notesRoot: input.notesRoot,
    relativePaths: paths,
    requestId: input.requestId
  };
}

export function parseSyncPathGuardRelease(
  value: unknown,
  request?: SyncPathGuardRequest
): SyncPathGuardRelease | null {
  const input = record(value);
  if (!input || !canonicalUuid(input.requestId) || !canonicalNotesRoot(input.notesRoot)) return null;
  const paths = relativePaths(input.relativePaths);
  if (!paths) return null;
  if (
    request &&
    (
      request.requestId !== input.requestId ||
      normalizeComparablePath(request.notesRoot) !== normalizeComparablePath(input.notesRoot) ||
      request.relativePaths.length !== paths.length ||
      request.relativePaths.some((path, index) => path !== paths[index])
    )
  ) {
    return null;
  }

  return {
    notesRoot: input.notesRoot,
    relativePaths: paths,
    requestId: input.requestId
  };
}

export function absoluteSyncGuardPaths(notesRoot: string, paths: readonly string[]) {
  const root = notesRoot.replace(/[\\/]+$/u, "");
  return paths.map((path) => `${root}/${path}`);
}

export function guardedPathsForRequests(
  requests: ReadonlyMap<string, ReadonlySet<string>>
): ReadonlySet<string> {
  const guarded = new Set<string>();
  requests.forEach((paths) => paths.forEach((path) => guarded.add(path)));
  return guarded;
}

export function editorReadOnlyForPath(
  userReadOnly: boolean,
  path: string | null | undefined,
  guardedPaths: ReadonlySet<string>
) {
  if (userReadOnly) return true;
  const normalized = normalizeComparablePath(path);
  if (!normalized) return false;

  return [...guardedPaths].some((guarded) => normalizeComparablePath(guarded) === normalized);
}

export function editorReadOnlyForTarget(
  target: "main" | "side",
  mainPath: string | null | undefined,
  sidePath: string | null | undefined,
  userReadOnly: boolean,
  guardedPaths: ReadonlySet<string>
) {
  return editorReadOnlyForPath(
    userReadOnly,
    target === "side" ? sidePath : mainPath,
    guardedPaths
  );
}

export function syncSaveBlockedByGuardedPath(
  saveAs: boolean,
  path: string | null | undefined,
  guardedPaths: ReadonlySet<string>
) {
  return !saveAs && syncExistingDocumentWriteBlockedByGuardedPath(path, guardedPaths);
}

export function syncExistingDocumentWriteBlockedByGuardedPath(
  path: string | null | undefined,
  guardedPaths: ReadonlySet<string>
) {
  return editorReadOnlyForPath(false, path, guardedPaths);
}

function pathContains(ancestor: string, descendant: string) {
  return ancestor === descendant || descendant.startsWith(`${ancestor}/`);
}

export function syncMutationIntersectsGuardedPaths(
  mutation: SyncPathMutation,
  guardedPaths: ReadonlySet<string>
) {
  const mutationPaths = [mutation.sourcePath, mutation.destinationPath]
    .map(normalizeComparablePath)
    .filter((path): path is string => path !== null);
  const normalizedGuards = [...guardedPaths]
    .map(normalizeComparablePath)
    .filter((path): path is string => path !== null);

  return mutationPaths.some((path) => normalizedGuards.some((guarded) => (
    pathContains(path, guarded) || pathContains(guarded, path)
  )));
}
