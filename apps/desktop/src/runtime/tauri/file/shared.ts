import { debug, fileNameFromPath } from "@markra/shared";
import type {
  AppFileRuntime,
  CreateNativeMarkdownTreeFileOptions,
  ListNativeMarkdownFilesOptions,
  NativeMarkdownFileChangeHandler,
  NativeMarkdownFolderFile,
  NativeMarkdownTreeChangeHandler,
  SavedNativeMarkdownFile,
  SaveNativeMarkdownFileInput,
  WorkspaceSearchRequest,
  WorkspaceSearchResponse
} from "@markra/app/runtime";
import { listenNativeEvent } from "../events";
import { invokeNative } from "../invoke";

type NativeMarkdownFileHistoryEntry = Awaited<ReturnType<AppFileRuntime["listMarkdownFileHistory"]>>[number];
type NativeMarkdownFileHistoryFile = Awaited<ReturnType<AppFileRuntime["readMarkdownFileHistory"]>>;
type LoadNativeMarkdownFilesForPath = NonNullable<AppFileRuntime["loadMarkdownFilesForPath"]>;
type LoadNativeMarkdownFilesForPathOptions = NonNullable<Parameters<LoadNativeMarkdownFilesForPath>[1]>;
type WatchNativeMarkdownOptions = NonNullable<Parameters<AppFileRuntime["watchMarkdownFile"]>[3]>;

type NativeMarkdownFile = {
  content: string;
  name: string;
  path: string;
  sizeBytes: number;
};

type MarkdownFileResponse = {
  path: string;
  contents: string;
  sizeBytes: number;
};

type MarkdownFileHistoryEntryResponse = {
  id: string;
  createdAt: number;
  sizeBytes: number;
};

type MarkdownFileHistoryFileResponse = {
  id: string;
  contents: string;
};

type MarkdownFolderFileResponse = {
  createdAt?: number;
  kind?: "asset" | "attachment" | "file" | "folder";
  modifiedAt?: number;
  path: string;
  relativePath: string;
  sizeBytes?: number;
};

type MarkdownWorkspaceSearchResultResponse = Omit<WorkspaceSearchResponse["results"][number], "file"> & {
  file: MarkdownFolderFileResponse;
};

type MarkdownWorkspaceSearchResponse = Omit<WorkspaceSearchResponse, "results"> & {
  results: MarkdownWorkspaceSearchResultResponse[];
};

type MarkdownFileTreeLoadEventResponse = {
  done?: boolean;
  error?: string;
  files?: MarkdownFolderFileResponse[];
  requestId: string;
};

type MarkdownFileChangedPayload = {
  path: string;
};

type MarkdownTreeChangedPayload = {
  path: string;
  rootPath: string;
};

const markdownFileChangedEvent = "markra://file-changed";
const markdownTreeChangedEvent = "markra://tree-changed";
const markdownFileTreeLoadEvent = "markra://markdown-tree-load";

function isMarkdownTreeFilePath(path: string) {
  return /\.(md|markdown)$/i.test(path);
}

function isMarkdownTreeAssetPath(path: string) {
  return /\.(avif|bmp|gif|jpe?g|png|svg|webp)$/i.test(path);
}

function parentPathFromPath(path: string) {
  const lastSeparatorIndex = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (lastSeparatorIndex < 0) return ".";
  return path.slice(0, lastSeparatorIndex);
}

function treeRootPathFromPath(path: string) {
  if (isMarkdownTreeFilePath(path) || isMarkdownTreeAssetPath(path)) {
    return parentPathFromPath(path);
  }

  return path;
}

function normalizeNativeParentPath(path: string | null | undefined) {
  const trimmedPath = path?.trim();
  return trimmedPath ? trimmedPath : null;
}

function markdownFolderFileFromResponse(file: MarkdownFolderFileResponse): NativeMarkdownFolderFile {
  const mappedFile: NativeMarkdownFolderFile = {
    path: file.path,
    name: fileNameFromPath(file.path),
    relativePath: file.relativePath
  };

  if (typeof file.createdAt === "number") mappedFile.createdAt = file.createdAt;
  if (typeof file.modifiedAt === "number") mappedFile.modifiedAt = file.modifiedAt;
  if (typeof file.sizeBytes === "number") mappedFile.sizeBytes = file.sizeBytes;

  if (file.kind === "asset" || (!file.kind && isMarkdownTreeAssetPath(file.relativePath))) {
    mappedFile.kind = "asset";
  } else if (file.kind === "attachment") {
    mappedFile.kind = "attachment";
  } else if (file.kind === "folder" || (!file.kind && !isMarkdownTreeFilePath(file.relativePath))) {
    mappedFile.kind = "folder";
  }

  return mappedFile;
}

export async function readNativeMarkdownFile(path: string): Promise<NativeMarkdownFile> {
  debug(() => ["[markra-history] native read file start", { path }]);
  const file = await invokeNative<MarkdownFileResponse>("read_markdown_file", { path });
  debug(() => ["[markra-history] native read file success", {
    contentsChars: file.contents.length,
    path: file.path,
    sizeBytes: file.sizeBytes
  }]);

  return {
    path: file.path,
    name: fileNameFromPath(file.path),
    content: file.contents,
    sizeBytes: file.sizeBytes
  };
}

export async function listNativeMarkdownFileHistory(path: string): Promise<NativeMarkdownFileHistoryEntry[]> {
  debug(() => ["[markra-history] native list history start", { path }]);
  const entries = await invokeNative<MarkdownFileHistoryEntryResponse[]>("list_markdown_file_history", { path });
  debug(() => ["[markra-history] native list history success", {
    entryCount: entries.length,
    firstEntryId: entries[0]?.id ?? null,
    path
  }]);
  return entries;
}

export async function readNativeMarkdownFileHistory(
  path: string,
  id: string
): Promise<NativeMarkdownFileHistoryFile> {
  debug(() => ["[markra-history] native read history start", { historyId: id, path }]);
  const file = await invokeNative<MarkdownFileHistoryFileResponse>("read_markdown_file_history", { id, path });
  debug(() => ["[markra-history] native read history success", {
    contentsChars: file.contents.length,
    historyId: file.id,
    path
  }]);
  return file;
}

export async function listNativeMarkdownFilesForPath(
  path: string,
  options: ListNativeMarkdownFilesOptions = {}
): Promise<NativeMarkdownFolderFile[]> {
  const args: {
    globalIgnoreRules?: string | null;
    managedAttachmentFolder?: string | null;
    path: string;
  } = { path };
  if (options.globalIgnoreRules !== undefined) args.globalIgnoreRules = options.globalIgnoreRules;
  if (options.managedAttachmentFolder !== undefined) {
    args.managedAttachmentFolder = options.managedAttachmentFolder;
  }
  const files = await invokeNative<MarkdownFolderFileResponse[]>("list_markdown_files_for_path", args);
  return files.map(markdownFolderFileFromResponse);
}

let markdownFileTreeLoadRequestIndex = 0;

function nextMarkdownFileTreeLoadRequestId() {
  markdownFileTreeLoadRequestIndex += 1;
  return `markdown-tree-load-${Date.now()}-${markdownFileTreeLoadRequestIndex}`;
}

function canceledMarkdownFileTreeLoadError() {
  return new Error("Markdown file tree load was canceled.");
}

export async function loadNativeMarkdownFilesForPath(
  path: string,
  options: LoadNativeMarkdownFilesForPathOptions = {}
): Promise<NativeMarkdownFolderFile[]> {
  const requestId = nextMarkdownFileTreeLoadRequestId();
  const allFiles: NativeMarkdownFolderFile[] = [];
  let removeListener: (() => unknown) | null = null;
  let abortHandler: EventListener | null = null;
  let settled = false;

  const cleanup = (cancelNativeLoad: boolean) => {
    const currentRemoveListener = removeListener;
    removeListener = null;
    currentRemoveListener?.();

    if (options.signal && abortHandler) {
      options.signal.removeEventListener("abort", abortHandler);
      abortHandler = null;
    }

    if (cancelNativeLoad) invokeNative("cancel_markdown_files_load", { requestId }).catch(() => {});
  };

  return new Promise<NativeMarkdownFolderFile[]>((resolve, reject) => {
    const fail = (error: unknown, cancelNativeLoad = true) => {
      if (settled) return;
      settled = true;
      cleanup(cancelNativeLoad);
      reject(error);
    };
    const complete = () => {
      if (settled) return;
      settled = true;
      cleanup(false);
      resolve(allFiles);
    };

    abortHandler = () => fail(canceledMarkdownFileTreeLoadError());
    if (options.signal?.aborted) {
      fail(canceledMarkdownFileTreeLoadError());
      return;
    }
    options.signal?.addEventListener("abort", abortHandler);

    listenNativeEvent<MarkdownFileTreeLoadEventResponse>(markdownFileTreeLoadEvent, (event) => {
      const payload = event.payload;
      if (payload.requestId !== requestId) return;
      if (payload.error) {
        fail(new Error(payload.error), false);
        return;
      }

      const batchFiles = payload.files?.map(markdownFolderFileFromResponse) ?? [];
      if (batchFiles.length > 0) {
        allFiles.push(...batchFiles);
        options.onBatch?.(batchFiles);
      }
      if (payload.done) complete();
    }).then((listenerCleanup) => {
      if (settled) {
        listenerCleanup();
        return;
      }

      removeListener = listenerCleanup;
      const args: {
        globalIgnoreRules?: string | null;
        managedAttachmentFolder?: string | null;
        path: string;
        requestId: string;
      } = { path, requestId };
      if (options.globalIgnoreRules !== undefined) args.globalIgnoreRules = options.globalIgnoreRules;
      if (options.managedAttachmentFolder !== undefined) {
        args.managedAttachmentFolder = options.managedAttachmentFolder;
      }
      invokeNative("load_markdown_files_for_path", args).catch((error: unknown) => fail(error));
    }).catch((error: unknown) => fail(error, false));
  });
}

export async function searchNativeMarkdownFilesForPath({
  caseSensitive,
  currentDocument,
  globalIgnoreRules,
  maxMatches,
  maxMatchesPerFile,
  path,
  query
}: WorkspaceSearchRequest): Promise<WorkspaceSearchResponse> {
  const search = await invokeNative<MarkdownWorkspaceSearchResponse>("search_markdown_files_for_path", {
    caseSensitive: caseSensitive === true,
    currentDocumentContent: currentDocument?.content,
    currentDocumentPath: currentDocument?.path,
    globalIgnoreRules,
    maxMatches,
    maxMatchesPerFile,
    path,
    query
  });

  return {
    ...search,
    results: search.results.map((result) => ({
      ...result,
      file: markdownFolderFileFromResponse(result.file)
    }))
  };
}

export async function createNativeMarkdownTreeFile(
  rootPath: string,
  fileName: string,
  optionsOrParentPath: CreateNativeMarkdownTreeFileOptions | string | null = null
): Promise<NativeMarkdownFolderFile> {
  const options = typeof optionsOrParentPath === "object" && optionsOrParentPath !== null
    ? optionsOrParentPath
    : { parentPath: optionsOrParentPath };
  const args: {
    contents?: string;
    fileName: string;
    parentPath: string | null;
    rootPath: string;
  } = {
    fileName,
    parentPath: normalizeNativeParentPath(options.parentPath),
    rootPath
  };
  if (typeof options.contents === "string") args.contents = options.contents;

  return markdownFolderFileFromResponse(
    await invokeNative<MarkdownFolderFileResponse>("create_markdown_tree_file", args)
  );
}

export async function createNativeMarkdownTreeFolder(
  rootPath: string,
  folderName: string,
  parentPath: string | null = null
): Promise<NativeMarkdownFolderFile> {
  return markdownFolderFileFromResponse(await invokeNative<MarkdownFolderFileResponse>(
    "create_markdown_tree_folder",
    { folderName, parentPath: normalizeNativeParentPath(parentPath), rootPath }
  ));
}

export async function renameNativeMarkdownTreeFile(
  rootPath: string,
  path: string,
  fileName: string
): Promise<NativeMarkdownFolderFile> {
  return markdownFolderFileFromResponse(await invokeNative<MarkdownFolderFileResponse>(
    "rename_markdown_tree_file",
    { fileName, path, rootPath }
  ));
}

export async function moveNativeMarkdownTreeFile(
  rootPath: string,
  path: string,
  targetParentPath: string | null = null
): Promise<NativeMarkdownFolderFile> {
  return markdownFolderFileFromResponse(await invokeNative<MarkdownFolderFileResponse>(
    "move_markdown_tree_file",
    { path, rootPath, targetParentPath: normalizeNativeParentPath(targetParentPath) }
  ));
}

export async function deleteNativeMarkdownTreeFile(rootPath: string, path: string) {
  await invokeNative("delete_markdown_tree_file", { path, rootPath });
}

export async function saveNativeMarkdownFileInPlace({
  historyCursorId,
  path,
  skipHistorySnapshot,
  contents
}: SaveNativeMarkdownFileInput): Promise<SavedNativeMarkdownFile> {
  if (!path) throw new Error("Managed workspace Markdown files require a path before saving.");

  const writeArgs: {
    contents: string;
    historyCursorId?: string;
    path: string;
    skipHistorySnapshot?: boolean;
  } = { contents, path };
  if (historyCursorId?.trim()) writeArgs.historyCursorId = historyCursorId;
  if (skipHistorySnapshot === true) writeArgs.skipHistorySnapshot = true;

  await invokeNative("write_markdown_file", writeArgs);
  debug(() => ["[markra-history] native save markdown success", {
    skipHistorySnapshot: skipHistorySnapshot === true,
    targetPath: path
  }]);
  return { path, name: fileNameFromPath(path) };
}

export async function watchNativeMarkdownFile(
  path: string,
  onChange: NativeMarkdownFileChangeHandler,
  onTreeChange?: NativeMarkdownTreeChangeHandler,
  options: WatchNativeMarkdownOptions = {}
) {
  debug(() => ["[markra-history] native watch subscribe", { path }]);
  const unlistenFile = await listenNativeEvent<MarkdownFileChangedPayload>(markdownFileChangedEvent, (event) => {
    if (event.payload.path !== path) return;
    debug(() => ["[markra-history] native watch file event", {
      path: event.payload.path
    }]);
    onChange(event.payload.path);
  });
  let unlistenTree: (() => unknown) | null = null;

  try {
    if (onTreeChange) {
      const rootPath = parentPathFromPath(path);
      unlistenTree = await listenNativeEvent<MarkdownTreeChangedPayload>(markdownTreeChangedEvent, (event) => {
        if (event.payload.rootPath !== rootPath) return;
        debug(() => ["[markra-history] native watch tree event", {
          path: event.payload.path,
          rootPath: event.payload.rootPath
        }]);
        onTreeChange(event.payload.path);
      });
    }
    await invokeNative("watch_markdown_file", {
      ...(options.globalIgnoreRules !== undefined ? { globalIgnoreRules: options.globalIgnoreRules } : {}),
      ...(options.ignoreRootPath !== undefined ? { ignoreRootPath: options.ignoreRootPath } : {}),
      path
    });
    debug(() => ["[markra-history] native watch ready", { path }]);
  } catch (error) {
    debug(() => ["[markra-history] native watch failed", {
      error: error instanceof Error ? error.message : String(error),
      path
    }]);
    unlistenFile();
    unlistenTree?.();
    throw error;
  }

  return () => {
    debug(() => ["[markra-history] native watch unsubscribe", { path }]);
    unlistenFile();
    unlistenTree?.();
    invokeNative("unwatch_markdown_file", { path });
  };
}

export async function watchNativeMarkdownTree(
  path: string,
  onTreeChange: NativeMarkdownTreeChangeHandler,
  options: WatchNativeMarkdownOptions = {}
) {
  const rootPath = treeRootPathFromPath(path);
  const unlistenTree = await listenNativeEvent<MarkdownTreeChangedPayload>(markdownTreeChangedEvent, (event) => {
    if (event.payload.rootPath === rootPath) onTreeChange(event.payload.path);
  });

  try {
    await invokeNative("watch_markdown_tree", {
      ...(options.globalIgnoreRules !== undefined ? { globalIgnoreRules: options.globalIgnoreRules } : {}),
      rootPath: path
    });
  } catch (error) {
    unlistenTree();
    throw error;
  }

  return () => {
    unlistenTree();
    invokeNative("unwatch_markdown_tree", { rootPath: path });
  };
}
