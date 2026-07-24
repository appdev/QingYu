import { getAppRuntime } from "../../runtime";
import type { WorkspaceSearchRequest, WorkspaceSearchResponse } from "../workspace-search";

export type NativeMarkdownFile = {
  path: string;
  name: string;
  content: string;
  sizeBytes?: number;
};

export type NativeSettingsFile = {
  path: string;
  name: string;
  content: string;
};

export type NativeMarkdownFileHistoryEntry = {
  id: string;
  createdAt: number;
  sizeBytes: number;
};

export type NativeMarkdownFileHistoryFile = {
  id: string;
  contents: string;
};

export type NativeMarkdownFolderFile = {
  createdAt?: number;
  kind?: "asset" | "attachment" | "folder";
  modifiedAt?: number;
  path: string;
  name: string;
  relativePath: string;
  sizeBytes?: number;
};

export type TrashWorkspaceResourceInput = {
  modifiedAt: number;
  relativePath: string;
  sizeBytes: number;
};

export type TrashWorkspaceResourceResult = {
  error?: string;
  relativePath: string;
  status: "trashed" | "failed";
};

export type MarkdownIgnoreOptions = {
  globalIgnoreRules?: string | null;
};

export type ListNativeMarkdownFilesOptions = MarkdownIgnoreOptions & {
  managedAttachmentFolder?: string | null;
};

export type WatchNativeMarkdownOptions = MarkdownIgnoreOptions & {
  ignoreRootPath?: string | null;
};

export type LoadNativeMarkdownFilesForPathOptions = ListNativeMarkdownFilesOptions & {
  onBatch?: (files: NativeMarkdownFolderFile[]) => unknown;
  signal?: AbortSignal | null;
};

export type NativeMarkdownFolder = {
  path: string;
  name: string;
};

export type NativeMarkdownDropPoint = {
  left: number;
  top: number;
};

export type CreateNativeMarkdownTreeFileOptions = {
  contents?: string | null;
  parentPath?: string | null;
};

export type NativeMarkdownDroppedTarget =
  | {
      kind: "file";
      path: string;
      name: string;
    }
  | {
      kind: "folder";
      path: string;
      name: string;
    }
  | {
      kind: "image";
      path: string;
      name: string;
      point?: NativeMarkdownDropPoint;
    };

export type SaveNativeMarkdownFileInput = {
  defaultDirectory?: string | null;
  historyCursorId?: string;
  path: string | null;
  skipHistorySnapshot?: boolean;
  suggestedName: string;
  contents: string;
};

export type SaveNativeHtmlFileInput = {
  suggestedName: string;
  contents: string;
};

export type SaveNativePdfFileInput = {
  suggestedName: string;
  contents: string;
};

export type SaveNativeSettingsFileInput = {
  suggestedName: string;
  contents: string;
};

export type NativePandocExportFormat = "docx" | "epub" | "latex";

export type SaveNativePandocFileInput = {
  documentPath: string | null;
  format: NativePandocExportFormat;
  markdown: string;
  pandocArgs: string;
  pandocPath: string;
  suggestedName: string;
};

export type SavedNativeMarkdownFile = {
  path: string;
  name: string;
};

export type SavedNativeHtmlFile = {
  path: string;
  name: string;
};

export type SavedNativePdfFile = {
  path: string;
  name: string;
};

export type SavedNativeSettingsFile = {
  path: string;
  name: string;
};

export type SavedNativePandocFile = {
  path: string;
  name: string;
};

export type SaveNativeClipboardImageInput = {
  copyToStorage?: boolean;
  documentPath: string | null;
  fileName: string;
  folder: string;
  image: File;
  projectRootPath?: string | null;
};

export type SaveNativeClipboardAttachmentInput = {
  attachment: File;
  copyToStorage?: boolean;
  documentPath: string | null;
  folder: string;
  projectRootPath?: string | null;
};

export type OpenNativeMarkdownAttachmentInput = {
  documentPath?: string | null;
  rootPath: string | null;
  src: string;
};

export type DownloadNativeWebImageInput = {
  src: string;
};

export type NativeMarkdownPickerLabels = {
  title: string;
};

export type NativeLocalFile = {
  name: string;
  path: string;
};

export type SavedNativeClipboardImage = {
  alt: string;
  src: string;
};

export type SavedNativeClipboardAttachment = {
  label: string;
  src: string;
};

export type ImportNativeLocalFileInput = {
  copyToStorage: boolean;
  documentPath: string | null;
  file: NativeLocalFile;
  folder: string;
  projectRootPath?: string | null;
};

export type NativeMarkdownFileChangeHandler = (path: string) => unknown | Promise<unknown>;
export type NativeMarkdownTreeChangeHandler = (path: string) => unknown | Promise<unknown>;
export type NativeMarkdownFileDropHandler = (target: NativeMarkdownDroppedTarget) => unknown | Promise<unknown>;

export function takeNativeOpenedMarkdownPaths() {
  return getAppRuntime().files.takeOpenedMarkdownPaths();
}

export function listenNativeOpenedMarkdownPaths(onPaths: (paths: string[]) => unknown | Promise<unknown>) {
  return getAppRuntime().files.listenOpenedMarkdownPaths(onPaths);
}

export function readNativeMarkdownFile(path: string) {
  return getAppRuntime().files.readMarkdownFile(path);
}

export async function searchNativeMarkdownFilesForPath(
  request: WorkspaceSearchRequest
): Promise<WorkspaceSearchResponse | null> {
  const searchMarkdownFiles = getAppRuntime().files.searchMarkdownFiles;
  if (!searchMarkdownFiles) return null;

  return searchMarkdownFiles(request);
}

export function listNativeMarkdownFileHistory(path: string) {
  return getAppRuntime().files.listMarkdownFileHistory(path);
}

export function readNativeMarkdownFileHistory(path: string, id: string) {
  return getAppRuntime().files.readMarkdownFileHistory(path, id);
}

export function readNativeMarkdownTemplateFile(fileName: string) {
  return getAppRuntime().files.readMarkdownTemplateFile(fileName);
}

export function writeNativeMarkdownTemplateFile(fileName: string, contents: string) {
  return getAppRuntime().files.writeMarkdownTemplateFile(fileName, contents);
}

export function deleteNativeMarkdownTemplateFile(fileName: string) {
  return getAppRuntime().files.deleteMarkdownTemplateFile(fileName);
}

export function readNativeLocalImageFile(path: string) {
  return getAppRuntime().files.readLocalImageFile(path);
}

export function listNativeMarkdownFilesForPath(path: string, options: ListNativeMarkdownFilesOptions = {}) {
  return getAppRuntime().files.listMarkdownFilesForPath(path, options);
}

export async function loadNativeMarkdownFilesForPath(
  path: string,
  options: LoadNativeMarkdownFilesForPathOptions = {}
) {
  const loadMarkdownFilesForPath = getAppRuntime().files.loadMarkdownFilesForPath;
  if (loadMarkdownFilesForPath) return loadMarkdownFilesForPath(path, options);

  const files = await listNativeMarkdownFilesForPath(path, {
    globalIgnoreRules: options.globalIgnoreRules,
    managedAttachmentFolder: options.managedAttachmentFolder
  });
  if (!options.signal?.aborted) options.onBatch?.(files);
  return files;
}

export function createNativeMarkdownTreeFile(
  rootPath: string,
  fileName: string,
  optionsOrParentPath: CreateNativeMarkdownTreeFileOptions | string | null = null
) {
  return getAppRuntime().files.createMarkdownTreeFile(rootPath, fileName, optionsOrParentPath);
}

export function createNativeMarkdownTreeFolder(rootPath: string, folderName: string, parentPath: string | null = null) {
  return getAppRuntime().files.createMarkdownTreeFolder(rootPath, folderName, parentPath);
}

export function renameNativeMarkdownTreeFile(rootPath: string, path: string, fileName: string) {
  return getAppRuntime().files.renameMarkdownTreeFile(rootPath, path, fileName);
}

export function moveNativeMarkdownTreeFile(rootPath: string, path: string, targetParentPath: string | null = null) {
  return getAppRuntime().files.moveMarkdownTreeFile(rootPath, path, targetParentPath);
}

export function deleteNativeMarkdownTreeFile(rootPath: string, path: string) {
  return getAppRuntime().files.deleteMarkdownTreeFile(rootPath, path);
}

export function openNativeContainingFolder(path: string) {
  return getAppRuntime().files.openContainingFolder(path);
}

export function openNativeMarkdownAttachment(input: OpenNativeMarkdownAttachmentInput) {
  return getAppRuntime().files.openMarkdownAttachment(input);
}

export function confirmNativeMarkdownFileDelete(
  fileName: string,
  labels: { cancelLabel: string; message: string; okLabel: string }
) {
  return getAppRuntime().files.confirmMarkdownFileDelete(fileName, labels);
}

export function confirmNativeUnsavedMarkdownDocumentDiscard(
  fileName: string,
  labels: { cancelLabel: string; message: string; okLabel: string }
) {
  return getAppRuntime().files.confirmUnsavedMarkdownDocumentDiscard(fileName, labels);
}

export function openNativeMarkdownFileInNewWindow(path: string) {
  return getAppRuntime().files.openMarkdownFileInNewWindow(path);
}

export function openNativeMarkdownFile(labels?: NativeMarkdownPickerLabels) {
  return getAppRuntime().files.openMarkdownFile(labels);
}

export function openNativeLocalImages(labels?: NativeMarkdownPickerLabels) {
  return getAppRuntime().files.openLocalImages(labels);
}

export function openNativeLocalFiles(labels?: NativeMarkdownPickerLabels) {
  return getAppRuntime().files.openLocalFiles(labels);
}

export function importNativeLocalFile(input: ImportNativeLocalFileInput) {
  return getAppRuntime().files.importLocalFile(input);
}

export function openNativeSettingsFile(labels?: NativeMarkdownPickerLabels) {
  return getAppRuntime().files.openSettingsFile(labels);
}

export function resolveNativeMarkdownPath(path: string) {
  return getAppRuntime().files.resolveMarkdownPath(path);
}

export function resolveNativeMarkdownFolder(path: string) {
  return getAppRuntime().files.resolveMarkdownFolder(path);
}

export function openNativeMarkdownFolder(labels?: NativeMarkdownPickerLabels) {
  return getAppRuntime().files.openMarkdownFolder(labels);
}

export function saveNativeMarkdownFile(input: SaveNativeMarkdownFileInput) {
  return getAppRuntime().files.saveMarkdownFile(input);
}

export function saveNativeHtmlFile(input: SaveNativeHtmlFileInput) {
  return getAppRuntime().files.saveHtmlFile(input);
}

export function saveNativePdfFile(input: SaveNativePdfFileInput) {
  return getAppRuntime().files.savePdfFile(input);
}

export function saveNativeSettingsFile(input: SaveNativeSettingsFileInput) {
  return getAppRuntime().files.saveSettingsFile(input);
}

export function saveNativePandocFile(input: SaveNativePandocFileInput) {
  return getAppRuntime().files.savePandocFile(input);
}

export function detectNativePandocPath() {
  return getAppRuntime().files.detectPandocPath();
}

export function saveNativeClipboardImage(input: SaveNativeClipboardImageInput) {
  return getAppRuntime().files.saveClipboardImage(input);
}

export function saveNativeClipboardAttachment(input: SaveNativeClipboardAttachmentInput) {
  return getAppRuntime().files.saveClipboardAttachment(input);
}

export function watchNativeMarkdownFile(
  path: string,
  onChange: NativeMarkdownFileChangeHandler,
  onTreeChange?: NativeMarkdownTreeChangeHandler,
  options: WatchNativeMarkdownOptions = {}
) {
  return getAppRuntime().files.watchMarkdownFile(path, onChange, onTreeChange, options);
}

export function watchNativeMarkdownTree(
  path: string,
  onTreeChange: NativeMarkdownTreeChangeHandler,
  options: WatchNativeMarkdownOptions = {}
) {
  return getAppRuntime().files.watchMarkdownTree(path, onTreeChange, options);
}

export function installNativeMarkdownFileDrop(onDrop: NativeMarkdownFileDropHandler) {
  return getAppRuntime().files.installMarkdownFileDrop(onDrop);
}
