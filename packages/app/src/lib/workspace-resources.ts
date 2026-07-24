import type { MarkdownResourceReference } from "@markra/markdown";
import type { NativeMarkdownFolderFile } from "./tauri/file";

export type WorkspaceMarkdownFile = NativeMarkdownFolderFile & { kind?: undefined };

export type WorkspaceResourceFile = NativeMarkdownFolderFile & {
  kind: "asset" | "attachment";
  modifiedAt: number;
  sizeBytes: number;
};

export type WorkspaceResourceOccurrence = MarkdownResourceReference & {
  sourceFile: WorkspaceMarkdownFile;
};

export type WorkspaceMissingResource = {
  href: string;
  occurrences: WorkspaceResourceOccurrence[];
  relativePath: string;
};

export type WorkspaceExistingResource = WorkspaceResourceFile & {
  referenceCount: number;
};

export type WorkspaceResourceFailure = {
  message: string;
  path: string;
  stage: "read" | "parse";
};

export type WorkspaceResourceGraph = {
  complete: boolean;
  existing: WorkspaceExistingResource[];
  failures: WorkspaceResourceFailure[];
  missing: WorkspaceMissingResource[];
  unused: WorkspaceExistingResource[];
};

const uriSchemePattern = /^[a-z][a-z0-9+.-]*:/iu;
const windowsAbsolutePathPattern = /^[a-z]:[\\/]/iu;
const markdownDocumentPattern = /\.(?:md|markdown)$/iu;

function normalizedSlashes(path: string) {
  const normalized = path.replace(/\\/gu, "/");
  if (/^\/\/\?\/UNC\//iu.test(normalized)) {
    return `//${normalized.slice("//?/UNC/".length)}`;
  }
  return normalized.replace(/^\/\/\?\//iu, "");
}

function normalizedRelativeParts(parts: readonly string[], initial: readonly string[] = []) {
  const normalized = [...initial];
  for (const part of parts) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (normalized.length === 0) return null;
      normalized.pop();
      continue;
    }
    normalized.push(part);
  }
  return normalized;
}

function normalizeRelativePath(path: string) {
  const value = normalizedSlashes(path.trim());
  if (!value || value.startsWith("/") || windowsAbsolutePathPattern.test(value)) return null;
  const parts = normalizedRelativeParts(value.split("/"));
  return parts && parts.length > 0 ? parts.join("/") : null;
}

function isWindowsAbsolutePath(path: string) {
  return windowsAbsolutePathPattern.test(path) || path.startsWith("//");
}

function normalizeAbsolutePath(path: string) {
  const value = normalizedSlashes(path.trim()).replace(/\/+$/u, "");
  if (!value) return null;

  if (windowsAbsolutePathPattern.test(value)) {
    const prefix = value.slice(0, 2);
    const parts = normalizedRelativeParts(value.slice(2).split("/"));
    return parts ? `${prefix}/${parts.join("/")}`.replace(/\/$/u, "") : null;
  }
  if (value.startsWith("//")) {
    const parts = normalizedRelativeParts(value.slice(2).split("/"));
    return parts && parts.length >= 2 ? `//${parts.join("/")}` : null;
  }
  if (value.startsWith("/")) {
    const parts = normalizedRelativeParts(value.slice(1).split("/"));
    return parts ? `/${parts.join("/")}`.replace(/\/$/u, "") || "/" : null;
  }

  return null;
}

function decodeResourceHref(href: string) {
  const value = href.trim();
  if (!value || value.startsWith("#") || value.startsWith("//")) return null;
  if (uriSchemePattern.test(value) && !windowsAbsolutePathPattern.test(value)) return null;

  const queryIndex = value.search(/[?#]/u);
  const path = queryIndex < 0 ? value : value.slice(0, queryIndex);
  if (!path || path.endsWith("/")) return null;

  try {
    const decoded = decodeURIComponent(path);
    return markdownDocumentPattern.test(decoded) ? null : decoded;
  } catch {
    return null;
  }
}

function isWindowsWorkspace(workspaceRoot: string) {
  const normalized = normalizedSlashes(workspaceRoot);
  return windowsAbsolutePathPattern.test(normalized) || normalized.startsWith("//");
}

function comparableRelativePath(path: string, caseInsensitive: boolean) {
  return caseInsensitive ? path.toLocaleLowerCase() : path;
}

function managedResourceRelativePath(relativePath: string, caseInsensitive: boolean) {
  const normalized = normalizeRelativePath(relativePath);
  if (!normalized) return false;

  const parts = normalized.split("/");
  if (parts.length <= 1) return false;
  return parts.slice(0, -1).some((part) => caseInsensitive
    ? part.toLocaleLowerCase() === "assets"
    : part === "assets");
}

export function isManagedResourceRelativePath(relativePath: string) {
  return managedResourceRelativePath(relativePath, false);
}

function workspaceRelativePathFromAbsolute(
  absolutePath: string,
  workspaceRoot: string,
  caseInsensitive: boolean
) {
  const normalizedTarget = normalizeAbsolutePath(absolutePath);
  const normalizedRoot = normalizeAbsolutePath(workspaceRoot);
  if (!normalizedTarget || !normalizedRoot) return null;

  const comparableTarget = caseInsensitive ? normalizedTarget.toLocaleLowerCase() : normalizedTarget;
  const comparableRoot = caseInsensitive ? normalizedRoot.toLocaleLowerCase() : normalizedRoot;
  const rootPrefix = `${comparableRoot.replace(/\/+$/u, "")}/`;
  if (!comparableTarget.startsWith(rootPrefix)) return null;

  return normalizedTarget.slice(normalizedRoot.replace(/\/+$/u, "").length + 1);
}

function resolveResourceRelativePath(
  href: string,
  sourceFile: WorkspaceMarkdownFile,
  workspaceRoot: string,
  caseInsensitive: boolean
) {
  const decoded = decodeResourceHref(href);
  if (!decoded) return null;

  const normalizedHref = normalizedSlashes(decoded);
  if (normalizedHref.startsWith("/") || isWindowsAbsolutePath(normalizedHref)) {
    const absoluteRelativePath = workspaceRelativePathFromAbsolute(
      normalizedHref,
      workspaceRoot,
      caseInsensitive
    );
    return absoluteRelativePath && managedResourceRelativePath(absoluteRelativePath, caseInsensitive)
      ? normalizeRelativePath(absoluteRelativePath)
      : null;
  }

  const sourceRelativePath = normalizeRelativePath(sourceFile.relativePath);
  if (!sourceRelativePath) return null;
  const sourceParts = sourceRelativePath.split("/");
  sourceParts.pop();
  const resolvedParts = normalizedRelativeParts(normalizedHref.split("/"), sourceParts);
  if (!resolvedParts || resolvedParts.length === 0) return null;

  const relativePath = resolvedParts.join("/");
  return managedResourceRelativePath(relativePath, caseInsensitive) ? relativePath : null;
}

function compareResourcePaths(left: string, right: string) {
  return left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" });
}

function compareOccurrences(left: WorkspaceResourceOccurrence, right: WorkspaceResourceOccurrence) {
  return compareResourcePaths(left.sourceFile.relativePath, right.sourceFile.relativePath)
    || left.lineNumber - right.lineNumber
    || left.columnNumber - right.columnNumber;
}

export function buildWorkspaceResourceGraph(input: {
  complete: boolean;
  failures: readonly WorkspaceResourceFailure[];
  markdownFiles: readonly WorkspaceMarkdownFile[];
  occurrences: ReadonlyMap<string, readonly MarkdownResourceReference[]>;
  resources: readonly WorkspaceResourceFile[];
  workspaceRoot: string;
}): WorkspaceResourceGraph {
  const caseInsensitive = isWindowsWorkspace(input.workspaceRoot);
  const referenceCounts = new Map<string, number>();
  const managedResources = input.resources.filter((resource) =>
    managedResourceRelativePath(resource.relativePath, caseInsensitive)
  );
  const resourceByIdentity = new Map<string, WorkspaceResourceFile>();
  const missingByIdentity = new Map<string, WorkspaceMissingResource>();

  managedResources.forEach((resource) => {
    const normalized = normalizeRelativePath(resource.relativePath);
    if (!normalized) return;
    const identity = comparableRelativePath(normalized, caseInsensitive);
    if (!resourceByIdentity.has(identity)) resourceByIdentity.set(identity, resource);
  });

  input.markdownFiles.forEach((sourceFile) => {
    const sourceOccurrences = input.occurrences.get(sourceFile.path) ?? [];
    sourceOccurrences.forEach((reference) => {
      const relativePath = resolveResourceRelativePath(
        reference.href,
        sourceFile,
        input.workspaceRoot,
        caseInsensitive
      );
      if (!relativePath) return;

      const identity = comparableRelativePath(relativePath, caseInsensitive);
      if (resourceByIdentity.has(identity)) {
        referenceCounts.set(identity, (referenceCounts.get(identity) ?? 0) + 1);
        return;
      }

      const occurrence: WorkspaceResourceOccurrence = { ...reference, sourceFile };
      const missing = missingByIdentity.get(identity);
      if (missing) {
        missing.occurrences.push(occurrence);
      } else {
        missingByIdentity.set(identity, {
          href: reference.href,
          occurrences: [occurrence],
          relativePath
        });
      }
    });
  });

  const existing = Array.from(resourceByIdentity.entries())
    .map(([identity, resource]) => ({
      ...resource,
      referenceCount: referenceCounts.get(identity) ?? 0
    }))
    .sort((left, right) => compareResourcePaths(left.relativePath, right.relativePath));
  const complete = input.complete && input.failures.length === 0;
  const unused = complete ? existing.filter((resource) => resource.referenceCount === 0) : [];
  const missing = Array.from(missingByIdentity.values())
    .map((resource) => ({
      ...resource,
      occurrences: resource.occurrences.sort(compareOccurrences)
    }))
    .sort((left, right) => compareResourcePaths(left.relativePath, right.relativePath));

  return {
    complete,
    existing,
    failures: [...input.failures],
    missing,
    unused
  };
}
