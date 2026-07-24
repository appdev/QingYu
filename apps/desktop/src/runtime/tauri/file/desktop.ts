import { invokeNative } from "../invoke";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { debug, fileNameFromPath } from "@markra/shared";
import { listenNativeEvent } from "../events";
import type {
  ImportNativeLocalFileInput,
  TrashWorkspaceResourceInput,
  TrashWorkspaceResourceResult
} from "@markra/app/runtime";
import {
  readNativeMarkdownFile,
  saveNativeMarkdownFileInPlace,
} from "./shared";

export {
  createNativeMarkdownTreeFile,
  createNativeMarkdownTreeFolder,
  deleteNativeMarkdownTreeFile,
  listNativeMarkdownFileHistory,
  listNativeMarkdownFilesForPath,
  loadNativeMarkdownFilesForPath,
  moveNativeMarkdownTreeFile,
  readNativeMarkdownFile,
  readNativeMarkdownFileHistory,
  renameNativeMarkdownTreeFile,
  saveNativeMarkdownFileInPlace,
  searchNativeMarkdownFilesForPath,
  watchNativeMarkdownFile,
  watchNativeMarkdownTree
} from "./shared";
export {
  confirmNativeMarkdownFileDelete,
  confirmNativeWorkspaceResourceTrash,
  confirmNativeUnsavedMarkdownDocumentDiscard
} from "./confirm";

type MarkdownTemplateFileResponse = {
  contents: string;
};

type TextFileResponse = {
  path: string;
  contents: string;
};

type MarkdownOpenPathResponse =
  | {
      kind: "file";
      path: string;
    }
  | {
      kind: "folder";
      path: string;
    };

export type NativeMarkdownFile = {
  path: string;
  name: string;
  content: string;
  sizeBytes: number;
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

export type NativeMarkdownPickerLabels = {
  title: string;
};

export type SavedNativeClipboardImage = {
  alt: string;
  src: string;
};

export type SavedNativeClipboardAttachment = {
  label: string;
  src: string;
};

export type NativeMarkdownFileChangeHandler = (path: string) => unknown | Promise<unknown>;
export type NativeMarkdownTreeChangeHandler = (path: string) => unknown | Promise<unknown>;
export type NativeMarkdownFileDropHandler = (target: NativeMarkdownDroppedTarget) => unknown | Promise<unknown>;

type NativeDragDropPositionPayload = {
  position?: unknown;
};

type NativeDragDropPathPayload = NativeDragDropPositionPayload & {
  paths?: unknown;
};

type NativeDragDropEventPayload =
  | {
      paths: string[];
      position?: unknown;
      type: "enter" | "drop";
    }
  | {
      position?: unknown;
      type: "over";
    }
  | {
      type: "leave";
    };

type ClipboardImageFileResponse = {
  relativePath: string;
};

type MarkdownImageFileResponse = {
  bytes: number[];
  mimeType: string;
  path: string;
};

type OpenedMarkdownPathsPayload = {
  paths?: unknown;
};

const openedMarkdownPathsEvent = "markra://opened-markdown-paths";

const markdownFilters = [
  {
    name: "Markdown",
    extensions: ["md", "markdown", "txt"]
  }
];

const htmlFilters = [
  {
    name: "HTML",
    extensions: ["html", "htm"]
  }
];

const imageFilters = [
  {
    name: "Images",
    extensions: ["avif", "bmp", "gif", "jpeg", "jpg", "png", "svg", "webp"]
  }
];
const imageFileExtensions = new Set(imageFilters.flatMap((filter) => filter.extensions));

const pdfFilters = [
  {
    name: "PDF",
    extensions: ["pdf"]
  }
];

const settingsFilters = [
  {
    name: "QingYu settings",
    extensions: ["json"]
  }
];

const pandocExportFilters: Record<NativePandocExportFormat, Array<{ extensions: string[]; name: string }>> = {
  docx: [
    {
      name: "Word document",
      extensions: ["docx"]
    }
  ],
  epub: [
    {
      name: "EPUB",
      extensions: ["epub"]
    }
  ],
  latex: [
    {
      name: "LaTeX",
      extensions: ["tex"]
    }
  ]
};

function normalizeOpenedMarkdownPaths(paths: unknown) {
  if (!Array.isArray(paths)) return [];

  return paths.filter((path): path is string => typeof path === "string" && path.trim().length > 0);
}

function nativePathSeparator(path: string) {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

function nativeDefaultSavePath(defaultDirectory: string | null | undefined, suggestedName: string) {
  const directory = defaultDirectory?.trim();
  const name = suggestedName.trim() || "Untitled.md";
  if (!directory) return name;

  const separator = nativePathSeparator(directory);
  return `${directory.replace(/[\\/]+$/u, "")}${separator}${name.replace(/^[\\/]+/u, "")}`;
}

export async function takeNativeOpenedMarkdownPaths(): Promise<string[]> {
  return normalizeOpenedMarkdownPaths(await invokeNative("take_opened_markdown_paths"));
}

export async function requestNativePrimaryNotebookSwitch(path: string) {
  await invokeNative("request_primary_notebook_switch", { path });
}

export async function listenNativeOpenedMarkdownPaths(onPaths: (paths: string[]) => unknown | Promise<unknown>) {
  return listenNativeEvent<OpenedMarkdownPathsPayload>(openedMarkdownPathsEvent, (event) => {
    const paths = normalizeOpenedMarkdownPaths(event.payload?.paths);
    if (paths.length === 0) return;

    Promise.resolve(onPaths(paths)).catch(() => {});
  });
}

export async function readNativeMarkdownTemplateFile(fileName: string): Promise<string> {
  const file = await invokeNative<MarkdownTemplateFileResponse>("read_markdown_template_file", {
    fileName
  });

  return file.contents;
}

export async function writeNativeMarkdownTemplateFile(fileName: string, contents: string) {
  await invokeNative("write_markdown_template_file", {
    contents,
    fileName
  });
}

export async function deleteNativeMarkdownTemplateFile(fileName: string) {
  await invokeNative("delete_markdown_template_file", {
    fileName
  });
}

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


export async function openNativeMarkdownFileInNewWindow(path: string) {
  await invokeNative("open_markdown_file_in_new_window", { path });
}

export async function openNativeContainingFolder(path: string) {
  await invokeNative("open_containing_folder", { path });
}

export async function openNativeMarkdownAttachment({
  documentPath = null,
  rootPath,
  src
}: OpenNativeMarkdownAttachmentInput) {
  await invokeNative("open_markdown_attachment", {
    documentPath,
    rootPath,
    src
  });
}

function pickerTitleOption(labels: NativeMarkdownPickerLabels | undefined) {
  const title = labels?.title.trim();
  return title ? { title } : {};
}

export async function openNativeMarkdownFile(labels?: NativeMarkdownPickerLabels): Promise<NativeMarkdownFile | null> {
  // The native dialog gives us a real filesystem path; Rust owns the actual disk read.
  const selectedPath = await open({
    multiple: false,
    fileAccessMode: "scoped",
    filters: markdownFilters,
    ...pickerTitleOption(labels)
  });

  if (!selectedPath || Array.isArray(selectedPath)) return null;

  return readNativeMarkdownFile(selectedPath);
}

function normalizeSelectedPaths(paths: string | string[] | null) {
  if (!paths) return [];

  return Array.isArray(paths) ? paths : [paths];
}

function nativeFileFromBytes(bytes: number[], path: string, mimeType: string) {
  const file = new File([new Uint8Array(bytes)], fileNameFromPath(path), {
    type: mimeType
  });

  Object.defineProperty(file, "path", {
    configurable: true,
    value: path
  });

  return file;
}

export async function openNativeLocalImages(labels?: NativeMarkdownPickerLabels): Promise<File[]> {
  const selectedPaths = normalizeSelectedPaths(await open({
    multiple: true,
    fileAccessMode: "scoped",
    filters: imageFilters,
    ...pickerTitleOption(labels)
  }));

  const images: File[] = [];
  for (const path of selectedPaths) {
    images.push(await readNativeLocalImageFile(path));
  }

  return images;
}

export async function openNativeLocalFiles(labels?: NativeMarkdownPickerLabels) {
  const selectedPaths = normalizeSelectedPaths(await open({
    multiple: true,
    fileAccessMode: "scoped",
    ...pickerTitleOption(labels)
  }));

  return selectedPaths.map((path) => ({
    name: fileNameFromPath(path),
    path
  }));
}

export async function readNativeLocalImageFile(path: string): Promise<File> {
  const image = await invokeNative<MarkdownImageFileResponse>("read_local_image_file", {
    path
  });

  return nativeFileFromBytes(image.bytes, image.path, image.mimeType);
}

export async function openNativeSettingsFile(labels?: NativeMarkdownPickerLabels): Promise<NativeSettingsFile | null> {
  const selectedPath = await open({
    multiple: false,
    fileAccessMode: "scoped",
    filters: settingsFilters,
    ...pickerTitleOption(labels)
  });

  if (!selectedPath || Array.isArray(selectedPath)) return null;

  const file = await invokeNative<TextFileResponse>("read_text_file", {
    path: selectedPath
  });

  return {
    path: file.path,
    name: fileNameFromPath(file.path),
    content: file.contents
  };
}

function droppedTargetFromResponse(target: MarkdownOpenPathResponse): NativeMarkdownDroppedTarget {
  return {
    kind: target.kind,
    path: target.path,
    name: fileNameFromPath(target.path)
  };
}

export async function resolveNativeMarkdownPath(path: string): Promise<NativeMarkdownDroppedTarget> {
  const target = await invokeNative<MarkdownOpenPathResponse>("resolve_markdown_path", {
    path
  });

  return droppedTargetFromResponse(target);
}

export async function resolveNativeWorkspaceResourceRoot(sourcePath: string) {
  return invokeNative<string>("resolve_workspace_resource_root", { sourcePath });
}

function normalizeTrashWorkspaceResourceResults(value: unknown): TrashWorkspaceResourceResult[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) => {
    if (typeof item !== "object" || item === null) return [];
    const candidate = item as Partial<TrashWorkspaceResourceResult>;
    if (
      typeof candidate.relativePath !== "string" ||
      (candidate.status !== "trashed" && candidate.status !== "failed") ||
      (candidate.error !== undefined && typeof candidate.error !== "string")
    ) {
      return [];
    }

    return [{
      ...(candidate.error === undefined ? {} : { error: candidate.error }),
      relativePath: candidate.relativePath,
      status: candidate.status
    }];
  });
}

export async function trashNativeWorkspaceResources(
  rootPath: string,
  resources: readonly TrashWorkspaceResourceInput[]
) {
  const result = await invokeNative("trash_workspace_resources", { resources, rootPath });
  return normalizeTrashWorkspaceResourceResults(result);
}

function imageDropTargetFromPath(path: string, point?: NativeMarkdownDropPoint): NativeMarkdownDroppedTarget | null {
  const extension = path.split(/[\\/]/u).pop()?.split(".").pop()?.toLocaleLowerCase();
  if (!extension || !imageFileExtensions.has(extension)) return null;

  return {
    kind: "image",
    name: fileNameFromPath(path),
    path,
    ...(point ? { point } : {})
  };
}

function nativeDropPointFromPosition(position: unknown): NativeMarkdownDropPoint | undefined {
  if (!position || typeof position !== "object") return undefined;
  const coordinates = "Physical" in position && typeof position.Physical === "object" && position.Physical !== null
    ? position.Physical
    : position;
  const { x, y } = coordinates as { x?: unknown; y?: unknown };
  if (typeof x !== "number" || typeof y !== "number") return undefined;

  const scaleFactor = typeof window.devicePixelRatio === "number" && window.devicePixelRatio > 0
    ? window.devicePixelRatio
    : 1;

  return {
    left: x / scaleFactor,
    top: y / scaleFactor
  };
}

function normalizeNativeDropPaths(paths: unknown) {
  if (!Array.isArray(paths)) return [];

  return paths.filter((path): path is string => typeof path === "string");
}

async function firstDroppedMarkdownTarget(paths: string[], point?: NativeMarkdownDropPoint) {
  for (const path of paths) {
    try {
      return droppedTargetFromResponse(await invokeNative<MarkdownOpenPathResponse>("resolve_markdown_path", {
        path
      }));
    } catch {
      const imageTarget = imageDropTargetFromPath(path, point);
      if (imageTarget) return imageTarget;
      // Keep looking; drag payloads can contain unsupported files.
    }
  }

  return null;
}

async function cleanupNativeEventListeners(cleanups: Array<() => unknown>) {
  await Promise.all(cleanups.map(async (cleanup) => {
    await cleanup();
  }));
}

async function listenNativeWindowDragDropEvent(handler: (event: { payload: NativeDragDropEventPayload }) => unknown) {
  const currentWindow = getCurrentWindow();
  const target = { kind: "Window" as const, label: currentWindow.label };
  const cleanups: Array<() => unknown> = [];

  try {
    // Tauri's composite onDragDropEvent cleanup can leak stale listener rejections from its internal unlisten calls.
    cleanups.push(await listenNativeEvent<NativeDragDropPathPayload>("tauri://drag-enter", (event) => {
      handler({
        payload: {
          paths: normalizeNativeDropPaths(event.payload.paths),
          position: event.payload.position,
          type: "enter"
        }
      });
    }, { target }));
    cleanups.push(await listenNativeEvent<NativeDragDropPositionPayload>("tauri://drag-over", (event) => {
      handler({
        payload: {
          position: event.payload.position,
          type: "over"
        }
      });
    }, { target }));
    cleanups.push(await listenNativeEvent<NativeDragDropPathPayload>("tauri://drag-drop", (event) => {
      handler({
        payload: {
          paths: normalizeNativeDropPaths(event.payload.paths),
          position: event.payload.position,
          type: "drop"
        }
      });
    }, { target }));
    cleanups.push(await listenNativeEvent<unknown>("tauri://drag-leave", () => {
      handler({
        payload: {
          type: "leave"
        }
      });
    }, { target }));
  } catch (error) {
    await cleanupNativeEventListeners(cleanups);
    throw error;
  }

  let cleaned = false;

  return async () => {
    if (cleaned) return;

    cleaned = true;
    await cleanupNativeEventListeners(cleanups);
  };
}

export async function openNativeMarkdownFolder(labels?: NativeMarkdownPickerLabels): Promise<NativeMarkdownFolder | null> {
  const selectedPath = await open({
    multiple: false,
    directory: true,
    recursive: true,
    fileAccessMode: "scoped",
    ...pickerTitleOption(labels)
  });

  if (!selectedPath || Array.isArray(selectedPath)) return null;

  return resolveNativeMarkdownFolder(selectedPath);
}

export async function resolveNativeMarkdownFolder(path: string): Promise<NativeMarkdownFolder> {
  const canonicalPath = await invokeNative<string>("resolve_markdown_folder", { path });
  return {
    path: canonicalPath,
    name: fileNameFromPath(canonicalPath)
  };
}

export async function saveNativeMarkdownFile({
  defaultDirectory,
  historyCursorId,
  path,
  skipHistorySnapshot,
  suggestedName,
  contents
}: SaveNativeMarkdownFileInput): Promise<SavedNativeMarkdownFile | null> {
  debug(() => ["[markra-history] native save markdown start", {
    contentsChars: contents.length,
    path,
    skipHistorySnapshot: skipHistorySnapshot === true,
    suggestedName
  }]);
  // Existing files save in place. Untitled documents first ask macOS for a target path.
  const targetPath =
    path ??
    (await save({
      defaultPath: nativeDefaultSavePath(defaultDirectory, suggestedName),
      filters: markdownFilters
    }));

  if (!targetPath) {
    debug(() => ["[markra-history] native save markdown canceled", {
      suggestedName
    }]);
    return null;
  }

  return saveNativeMarkdownFileInPlace({
    contents,
    defaultDirectory,
    historyCursorId,
    path: targetPath,
    skipHistorySnapshot,
    suggestedName
  });
}

export async function saveNativeHtmlFile({
  suggestedName,
  contents
}: SaveNativeHtmlFileInput): Promise<SavedNativeHtmlFile | null> {
  const targetPath = await save({
    defaultPath: suggestedName,
    filters: htmlFilters
  });

  if (!targetPath) return null;

  await invokeNative("write_markdown_file", {
    path: targetPath,
    contents
  });

  return {
    path: targetPath,
    name: fileNameFromPath(targetPath)
  };
}

export async function saveNativePdfFile({
  suggestedName,
  contents
}: SaveNativePdfFileInput): Promise<SavedNativePdfFile | null> {
  const targetPath = await save({
    defaultPath: suggestedName,
    filters: pdfFilters
  });

  if (!targetPath) return null;

  await invokeNative("export_pdf_file", {
    path: targetPath,
    html: contents
  });

  return {
    path: targetPath,
    name: fileNameFromPath(targetPath)
  };
}

export async function saveNativeSettingsFile({
  suggestedName,
  contents
}: SaveNativeSettingsFileInput): Promise<SavedNativeSettingsFile | null> {
  const targetPath = await save({
    defaultPath: suggestedName,
    filters: settingsFilters
  });

  if (!targetPath) return null;

  await invokeNative("write_text_file", {
    path: targetPath,
    contents
  });

  return {
    path: targetPath,
    name: fileNameFromPath(targetPath)
  };
}

export async function saveNativePandocFile({
  documentPath,
  format,
  markdown,
  pandocArgs,
  pandocPath,
  suggestedName
}: SaveNativePandocFileInput): Promise<SavedNativePandocFile | null> {
  await invokeNative("check_pandoc_available", {
    pandocPath
  });

  const targetPath = await save({
    defaultPath: suggestedName,
    filters: pandocExportFilters[format]
  });

  if (!targetPath) return null;

  await invokeNative("export_pandoc_file", {
    documentPath,
    format,
    markdown,
    pandocArgs,
    pandocPath,
    path: targetPath
  });

  return {
    path: targetPath,
    name: fileNameFromPath(targetPath)
  };
}

export async function detectNativePandocPath(): Promise<string | null> {
  const path = await invokeNative<string | null>("detect_pandoc_path");
  const trimmedPath = typeof path === "string" ? path.trim() : "";

  return trimmedPath || null;
}

function imageAltFromFileName(fileName: string) {
  const trimmedName = fileName.trim();
  if (!trimmedName) return "image";

  const withoutExtension = trimmedName.replace(/\.[^.]*$/u, "").trim();
  return withoutExtension || "image";
}

function encodeMarkdownUrlSegment(segment: string) {
  return encodeURIComponent(segment).replace(/[!'()*]/gu, (character) =>
    `%${character.charCodeAt(0).toString(16).toUpperCase()}`
  );
}

function encodeMarkdownRelativePath(path: string) {
  return path.split("/").map(encodeMarkdownUrlSegment).join("/");
}

type NativeFilePath = File & {
  path?: unknown;
};

function nativeFilePath(file: File) {
  const path = (file as NativeFilePath).path;
  return typeof path === "string" && path.trim().length > 0 ? path.trim() : null;
}

function encodeFileUrlPathSegments(segments: string[]) {
  return segments.map(encodeMarkdownUrlSegment).join("/");
}

function fileUrlFromNativePath(path: string) {
  const normalized = path.trim().replace(/\\/gu, "/");
  if (!normalized) throw new Error("Clipboard file path is unavailable.");

  if (normalized.startsWith("//")) {
    const [host, ...segments] = normalized.slice(2).split("/");
    if (!host) throw new Error("Clipboard file path is unavailable.");

    return `file://${host}/${encodeFileUrlPathSegments(segments)}`;
  }

  if (/^[a-zA-Z]:\//u.test(normalized)) {
    const [drive, ...segments] = normalized.split("/");
    return `file:///${drive}/${encodeFileUrlPathSegments(segments)}`;
  }

  if (normalized.startsWith("/")) {
    const [, ...segments] = normalized.split("/");
    return `file:///${encodeFileUrlPathSegments(segments)}`;
  }

  throw new Error("Clipboard file path must be absolute.");
}

async function fileUrlFromNativeFile(file: File) {
  const path = nativeFilePath(file);
  if (!path) throw new Error("Clipboard file path is unavailable.");

  const canonicalPath = await invokeNative<string>("canonical_local_file_path", { path });
  return fileUrlFromNativePath(canonicalPath);
}

async function canonicalFileUrlFromNativePath(path: string) {
  const canonicalPath = await invokeNative<string>("canonical_local_file_path", { path });
  return fileUrlFromNativePath(canonicalPath);
}

export async function saveNativeClipboardImage({
  copyToStorage = true,
  documentPath,
  fileName,
  folder,
  image,
  projectRootPath = null
}: SaveNativeClipboardImageInput): Promise<SavedNativeClipboardImage> {
  if (!copyToStorage) {
    return {
      alt: imageAltFromFileName(image.name),
      src: await fileUrlFromNativeFile(image)
    };
  }

  if (!documentPath) throw new Error("Current document must be a saved Markdown file.");

  const bytes = Array.from(new Uint8Array(await image.arrayBuffer()));
  const request = {
    bytes,
    documentPath,
    fileName,
    folder: projectRootPath ? "assets" : folder,
    mimeType: image.type
  } as {
    bytes: number[];
    documentPath: string;
    fileName: string;
    folder: string;
    mimeType: string;
    projectRootPath?: string;
    sourcePath?: string | null;
  };
  if (projectRootPath) {
    request.projectRootPath = projectRootPath;
    request.sourcePath = nativeFilePath(image);
  }
  const savedImage = await invokeNative<ClipboardImageFileResponse>("save_clipboard_image", request);

  return {
    alt: imageAltFromFileName(image.name),
    src: encodeMarkdownRelativePath(savedImage.relativePath)
  };
}

export async function saveNativeClipboardAttachment({
  attachment,
  copyToStorage = true,
  documentPath,
  folder,
  projectRootPath = null
}: SaveNativeClipboardAttachmentInput): Promise<SavedNativeClipboardAttachment> {
  if (!copyToStorage) {
    return {
      label: attachment.name.trim() || fileNameFromPath(nativeFilePath(attachment) ?? "") || "attachment",
      src: await fileUrlFromNativeFile(attachment)
    };
  }

  if (!documentPath) throw new Error("Current document must be a saved Markdown file.");

  const bytes = Array.from(new Uint8Array(await attachment.arrayBuffer()));
  const request = {
    bytes,
    documentPath,
    fileName: attachment.name,
    folder: projectRootPath ? "assets" : folder
  } as {
    bytes: number[];
    documentPath: string;
    fileName: string;
    folder: string;
    projectRootPath?: string;
    sourcePath?: string | null;
  };
  if (projectRootPath) {
    request.projectRootPath = projectRootPath;
    request.sourcePath = nativeFilePath(attachment);
  }
  const savedAttachment = await invokeNative<ClipboardImageFileResponse>("save_clipboard_attachment", request);

  return {
    label: attachment.name.trim() || "attachment",
    src: encodeMarkdownRelativePath(savedAttachment.relativePath)
  };
}

export async function importNativeLocalFile({
  copyToStorage,
  documentPath,
  file,
  folder,
  projectRootPath = null
}: ImportNativeLocalFileInput): Promise<SavedNativeClipboardAttachment> {
  if (!copyToStorage) {
    return {
      label: file.name.trim() || fileNameFromPath(file.path) || "attachment",
      src: await canonicalFileUrlFromNativePath(file.path)
    };
  }

  if (!documentPath) throw new Error("Current document must be a saved Markdown file.");

  const request = {
    documentPath,
    folder: projectRootPath ? "assets" : folder,
    sourcePath: file.path
  } as {
    documentPath: string;
    folder: string;
    projectRootPath?: string;
    sourcePath: string;
  };
  if (projectRootPath) request.projectRootPath = projectRootPath;
  const savedAttachment = await invokeNative<ClipboardImageFileResponse>("import_local_file", request);

  return {
    label: file.name.trim() || "attachment",
    src: encodeMarkdownRelativePath(savedAttachment.relativePath)
  };
}


export async function installNativeMarkdownFileDrop(onDrop: NativeMarkdownFileDropHandler) {
  try {
    return await listenNativeWindowDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const point = nativeDropPointFromPosition(event.payload.position);

      firstDroppedMarkdownTarget(event.payload.paths, point).then((target) => {
        if (!target) return;

        onDrop(target);
      }).catch(() => {});
    });
  } catch {
    return () => {};
  }
}
