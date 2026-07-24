import type { EditorResourceOrigin } from "@markra/editor";

import { managedDocumentRelativePath } from "./settings/workspace-state";

export type EditorAssetContext =
  | { mode: "standalone" }
  | { mode: "primary-workspace"; primaryRootPath: string };

export type EditorAssetAction = "copy-document" | "copy-workspace" | "reference";

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
  primaryWorkspaceRoot
}: {
  documentPath: string | null;
  primaryWorkspaceRoot: string | null;
}): EditorAssetContext {
  if (
    documentPath &&
    primaryWorkspaceRoot &&
    managedDocumentRelativePath(primaryWorkspaceRoot, documentPath) !== null
  ) {
    return { mode: "primary-workspace", primaryRootPath: primaryWorkspaceRoot };
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
