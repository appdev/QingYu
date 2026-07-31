import type { EditorResourceOrigin } from "@markra/editor";

import { managedDocumentRelativePath } from "./settings/workspace-state";

export type EditorAssetContext =
  | { mode: "standalone" }
  | { mode: "primary-workspace"; primaryRootPath: string };

export type EditorAssetAction = "copy-document" | "copy-workspace" | "reference";

function managedUriDocumentRelativePath(rootPath: string, filePath: string): string | null {
  if (!/^[a-z][a-z\d+.-]*:\/\/[^/?#]+$/iu.test(rootPath)) return null;

  const prefix = `${rootPath}/`;
  if (!filePath.startsWith(prefix)) return null;
  const encodedRelativePath = filePath.slice(prefix.length);
  if (!encodedRelativePath || encodedRelativePath.endsWith("/")) return null;

  let segments: string[];
  try {
    segments = encodedRelativePath.split("/").map(decodeURIComponent);
  } catch {
    return null;
  }
  if (segments.some((segment) => (
    !segment ||
    segment === "." ||
    segment === ".." ||
    /[\u0000-\u001f\u007f-\u009f\\/]/u.test(segment)
  ))) return null;

  return /\.(?:md|markdown)$/iu.test(segments.at(-1) ?? "")
    ? encodedRelativePath
    : null;
}

function editorDocumentRelativePath(rootPath: string, filePath: string): string | null {
  if (/^[a-z][a-z\d+.-]*:\/\//iu.test(rootPath)) {
    return managedUriDocumentRelativePath(rootPath, filePath);
  }
  return managedDocumentRelativePath(rootPath, filePath);
}

export function resolveEditorAssetAction({
  mode,
  origin
}: {
  mode: EditorAssetContext["mode"];
  origin: EditorResourceOrigin;
}): EditorAssetAction {
  if (mode === "primary-workspace") return "copy-workspace";
  return origin === "clipboard" ? "copy-document" : "reference";
}

export function resolveEditorAssetContext({
  documentPath,
  managedWorkspaceRoot,
  primaryWorkspaceRoot
}: {
  documentPath: string | null;
  managedWorkspaceRoot?: string;
  primaryWorkspaceRoot: string | null;
}): EditorAssetContext {
  const authoritativeRoot = managedWorkspaceRoot ?? primaryWorkspaceRoot;
  if (
    documentPath &&
    authoritativeRoot &&
    editorDocumentRelativePath(authoritativeRoot, documentPath) !== null
  ) {
    return { mode: "primary-workspace", primaryRootPath: authoritativeRoot };
  }

  return { mode: "standalone" };
}

export async function persistRemoteEditorImage<TSaved extends { alt: string; src: string }>({
  alt,
  context,
  download,
  save,
  url
}: {
  alt: string;
  context: EditorAssetContext;
  download: (url: string) => Promise<File>;
  save: (file: File) => Promise<TSaved | null>;
  url: string;
}): Promise<TSaved | { alt: string; src: string } | null> {
  if (resolveEditorAssetAction({ mode: context.mode, origin: "remote" }) === "reference") {
    return { alt: alt || "image", src: url };
  }

  const image = await download(url);
  return save(image);
}
