import { invokeNative } from "./invoke";

export function resolveNativeManagedWorkspaceRoot(name: string) {
  return invokeNative<string | null>("resolve_managed_workspace_root", { name });
}

export function listNativeManagedWorkspaceNames() {
  return invokeNative<string[]>("list_managed_workspace_names");
}

export function isNativeDocumentInWorkspace(documentPath: string, rootPath: string) {
  return invokeNative<boolean>("is_document_in_workspace", { documentPath, rootPath });
}

export function prepareNativeDesktopNotebookTarget(input: {
  notebookName: string;
  parentPath: string;
}) {
  return invokeNative<{ lease: string; notesRoot: string }>("prepare_desktop_notebook_target", input);
}

export function discardNativePreparedDesktopNotebookTarget(lease: string) {
  return invokeNative<undefined>("discard_prepared_desktop_notebook_target", { lease });
}
