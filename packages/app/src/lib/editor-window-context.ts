export type EditorWindowContext =
  | { kind: "primary" }
  | { kind: "external-blank" }
  | { kind: "external-file"; path: string }
  | { kind: "external-folder"; path: string };

export function parseEditorWindowContext(search: string): EditorWindowContext {
  const params = new URLSearchParams(search);
  const filePath = params.get("path")?.trim();
  if (filePath) return { kind: "external-file", path: filePath };

  const folderPath = params.get("folder")?.trim();
  if (folderPath) return { kind: "external-folder", path: folderPath };

  if (params.has("blank")) return { kind: "external-blank" };

  return { kind: "primary" };
}

export function isExternalEditorWindow(context: EditorWindowContext) {
  return context.kind !== "primary";
}
