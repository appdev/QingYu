import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  clearStoredRecentMarkdownFiles,
  getStoredRecentMarkdownFiles,
  getStoredWorkspaceState,
  prependRecentMarkdownFile,
  removeStoredRecentMarkdownFile,
  saveStoredRecentMarkdownFile,
  saveStoredWorkspaceState,
  type RecentMarkdownFile,
  type StoredWorkspaceDraftTab,
  type StoredWorkspaceState,
  type StoredWorkspaceWindow
} from "../lib/settings/app-settings";
import { getMarkdownOutline, getWordCount, type MarkdownOutlineItem } from "@markra/markdown";
import {
  destroyNativeWindow,
  exitNativeApp,
  listNativeEditorWindowRestoreStates,
  openNativeMarkdownFile,
  openNativeMarkdownFolderInNewWindow,
  openNativeMarkdownFileInNewWindow,
  readNativeMarkdownFile,
  resolveNativeMarkdownPath,
  saveNativeMarkdownFile,
  setNativeEditorWindowRestoreState,
  listenNativeAppExitRequested,
  listenNativeWindowCloseRequested,
  listenNativeOpenedMarkdownPaths,
  takeNativeOpenedMarkdownPaths,
  watchNativeMarkdownFile,
  type NativeMarkdownDroppedTarget,
  type NativeMarkdownFile,
  type NativeMarkdownFolderFile,
  type SavedNativeMarkdownFile
} from "../lib/tauri";
import { shouldBlockLargeMarkdownVisual } from "../lib/large-markdown";
import { scheduleMarkdownSummaryIdle, shouldDeferMarkdownSummary } from "../lib/markdown-summary";
import {
  measureAppPerformance,
  measureAppPerformanceAsync
} from "../lib/performance-marks";
import { normalizeComparablePath, replaceMovedPath, sameNativePath } from "../lib/path-move";
import { setNativeWindowTitle } from "../lib/tauri";
import { debug, isMarkdownPath, parentPathFromPath, pathNameFromPath, type DocumentState } from "@markra/shared";
import {
  activeFilePathFromTabs,
  activeFilePathFromWindowRestore,
  blankDocumentName,
  createDocumentTab,
  createInitialDocumentState,
  defaultSaveDirectoryInput,
  documentFromDraftTab,
  documentFromTab,
  draftWorkspacePatchFromTabs,
  fileTabId,
  isDeletedDocumentPath,
  isEquivalentEditorMarkdown,
  isPristineUntitledDocument,
  normalizeOpenFilePaths,
  openFilePathsFromTabs,
  restoreFilePathsFromWorkspace,
  type MarkdownDocumentTab
} from "./markdown-document/document-model";
import { createEditorSyncState, type EditorSyncState } from "./markdown-document/editor-sync";
import {
  managedDocumentRelativePath
} from "../lib/settings/workspace-state";
import {
  kernelWorkspaceDocumentRelativePathFromPath,
  kernelWorkspaceRelativePathFromPath
} from "../runtime/kernel-app/app-config";
import { kernelWorkspaceRoot } from "../runtime/kernel-app/files";
import {
  parseEditorWindowContext,
  type EditorWindowContext
} from "../lib/editor-window-context";
import { workspaceSurfaceForRestore } from "./markdown-document/restore-outcome";

export type { MarkdownDocumentTab } from "./markdown-document/document-model";
export type WorkspaceSurface = "restoring" | "editor" | "home" | "recovery";

function isEmptyUntitledDocument(document: DocumentState) {
  return document.open && document.path === null && document.content.trim() === "";
}

function isKernelWorkspaceDirectory(path: string | null | undefined) {
  if (!path) return false;

  try {
    kernelWorkspaceRelativePathFromPath(path);
    return true;
  } catch {
    return false;
  }
}

type CreateBlankDocumentOptions = {
  content?: string;
  name?: string;
};

type MarkdownChangeOptions = {
  documentRevision?: number;
  surface?: "source" | "visual";
};

type SaveCurrentDocumentContentOptions = {
  historyCursorId?: string;
  skipHistorySnapshot?: boolean;
  syncPathGuardRequestId?: string;
};

type ApplySavedCurrentDocumentOptions = {
  retargetWorkspaceRoot?: boolean;
  sourceContent?: string;
  targetTabId?: string | null;
};

type MovedMarkdownDocumentUpdate = {
  content: string;
  dirty: boolean;
};

export type ActiveDiskFileContentChange = {
  content: string;
  path: string;
  previousContent: string;
  revision: number;
  source: string;
};

type MarkdownDocumentSummary = {
  key: string;
  outlineItems: MarkdownOutlineItem[];
  wordCount: number;
};

const emptyMarkdownDocumentSummary: Omit<MarkdownDocumentSummary, "key"> = {
  outlineItems: [],
  wordCount: 0
};

function markdownDocumentSummaryKey(document: DocumentState) {
  return [
    document.revision,
    document.content.length,
    document.sizeBytes ?? "unknown-size"
  ].join(":");
}

function isMissingMarkdownFileReadError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return /(?:cannot find|no longer exists|no such file|not found)/iu.test(message);
}

function calculateMarkdownDocumentSummary(
  content: string,
  detail: Record<string, unknown>
): Omit<MarkdownDocumentSummary, "key"> {
  return measureAppPerformance("markdown-summary", () => ({
    outlineItems: getMarkdownOutline(content),
    wordCount: getWordCount(content)
  }), detail);
}

type UseMarkdownDocumentOptions = {
  autoSaveEnabled?: boolean;
  autoSaveIntervalMinutes?: number;
  confirmDiscardUnsavedChanges?: (document: DocumentState) => boolean | Promise<boolean>;
  defaultSaveDirectory?: string | null;
  documentTabsEnabled?: boolean;
  editorReady?: boolean | (() => boolean);
  getCurrentMarkdown: (fallbackContent: string) => string;
  globalIgnoreRules?: string;
  isCurrentMarkdownEquivalent?: (markdown: string) => boolean | undefined;
  managedWorkspace?: boolean;
  nativeCloseBlocked?: boolean;
  nativeOpenPolicy?: "editor" | "managed" | "spawn-external";
  onActiveDiskFileContentChange?: (change: ActiveDiskFileContentChange) => boolean | undefined;
  onMarkdownTreeChange?: (path: string) => unknown | Promise<unknown>;
  onTreeRootFromFolderPath: (
    path: string,
    name: string,
    clearFilePath?: boolean,
    openTree?: boolean,
    options?: { persistWorkspace?: boolean }
  ) => unknown | Promise<unknown>;
  onTreeRootFromFilePath: (path: string) => unknown;
  onSwitchNotebookDirectory?: (path: string) => unknown | Promise<unknown>;
  openDroppedFilesInTabs?: boolean;
  preferencesReady?: boolean;
  restoreWorkspaceRoot?: string | null;
  restoreWorkspaceOnStartup?: boolean;
  saveAsWorkspacePolicy?:
    | { kind: "primary"; root: string | null }
    | { kind: "standalone" };
  workspaceSourcePath?: string | null;
  workspaceReady?: boolean;
  windowContext?: EditorWindowContext;
  workspacePersistencePolicy?: "shared" | "isolated";
};

type ClearOpenDocumentOptions = {
  openBlank?: boolean;
  persistWorkspace?: boolean;
};

type WatchedMarkdownFileReadState = {
  changedPath: string;
  pending: boolean;
  promise: Promise<unknown> | null;
};

type OpenMarkdownFileOptions = {
  pickerTitle?: string;
};
type OpenTreeMarkdownFileOptions = {
  managed?: boolean;
};

type NativeMarkdownFilesRestoreResult = {
  activeFilePath: string | null;
  cancelled: boolean;
  failedPaths: string[];
  files: NativeMarkdownFile[];
  survivingPaths: string[];
};

type WorkspaceDraftRestoreResult = {
  activeFilePath: string | null;
  activeTabId: string | null;
  recoverableDraftCount: number;
  tabs: MarkdownDocumentTab[];
};

function persistDesktopWorkspaceState(patch: Parameters<typeof saveStoredWorkspaceState>[0]) {
  return saveStoredWorkspaceState(patch).catch(() => {});
}

function managedWorkspaceStatePatch(patch: Partial<StoredWorkspaceState>): Partial<StoredWorkspaceState> {
  const nextPatch = { ...patch };

  nextPatch.folderName = null;
  nextPatch.folderPath = null;
  nextPatch.openWindows = [];
  nextPatch.sideBySideGroup = null;

  return nextPatch;
}

function documentPathIsWithinWorkspaceRoot(rootPath: string, filePath: string) {
  if (rootPath !== kernelWorkspaceRoot) {
    return managedDocumentRelativePath(rootPath, filePath) !== null;
  }

  try {
    kernelWorkspaceDocumentRelativePathFromPath(filePath);
    return true;
  } catch {
    return false;
  }
}

function resolveEditorReady(editorReady: boolean | (() => boolean)) {
  return typeof editorReady === "function" ? editorReady() : editorReady;
}

function isPathWithinRoot(path: string, rootPath: string | null) {
  const normalizedPath = normalizeComparablePath(path);
  const normalizedRootPath = normalizeComparablePath(rootPath);
  if (!normalizedPath || !normalizedRootPath) return false;
  if (normalizedPath === normalizedRootPath) return true;

  const rootWithSeparator = normalizedRootPath.endsWith("/")
    ? normalizedRootPath
    : `${normalizedRootPath}/`;
  return normalizedPath.startsWith(rootWithSeparator);
}

function workspaceRootForSource(sourcePath: string | null | undefined, currentFilePath: string | null) {
  const normalizedSourcePath = normalizeComparablePath(sourcePath);
  if (normalizedSourcePath) {
    return isMarkdownPath(normalizedSourcePath)
      ? parentPathFromPath(normalizedSourcePath)
      : normalizedSourcePath;
  }

  const normalizedCurrentFilePath = normalizeComparablePath(currentFilePath);
  return normalizedCurrentFilePath ? parentPathFromPath(normalizedCurrentFilePath) : null;
}

function workspaceHasCurrentWindowRestoreState(workspace: StoredWorkspaceState) {
  return Boolean(
    workspace.filePath ||
    workspace.folderPath ||
    workspace.openFilePaths.length > 0 ||
    workspace.draftTabs?.length
  );
}

function additionalEditorWindowsForRestore(
  restoreWindows: readonly StoredWorkspaceWindow[],
  currentWindowHasRestoreState: boolean
) {
  return currentWindowHasRestoreState
    ? restoreWindows.filter((window) => window.label !== "main")
    : restoreWindows.slice(1);
}

export function useMarkdownDocument({
  autoSaveEnabled = false,
  autoSaveIntervalMinutes = 10,
  confirmDiscardUnsavedChanges,
  defaultSaveDirectory,
  documentTabsEnabled = false,
  editorReady = true,
  getCurrentMarkdown,
  globalIgnoreRules = "",
  isCurrentMarkdownEquivalent,
  managedWorkspace = false,
  nativeCloseBlocked = false,
  nativeOpenPolicy,
  onActiveDiskFileContentChange,
  onMarkdownTreeChange,
  onTreeRootFromFolderPath,
  onTreeRootFromFilePath,
  onSwitchNotebookDirectory,
  openDroppedFilesInTabs = false,
  preferencesReady = true,
  restoreWorkspaceRoot = null,
  restoreWorkspaceOnStartup = true,
  saveAsWorkspacePolicy = { kind: "standalone" },
  workspaceReady = true,
  workspaceSourcePath,
  windowContext = parseEditorWindowContext(window.location.search),
  workspacePersistencePolicy = "shared"
}: UseMarkdownDocumentOptions) {
  const resolvedNativeOpenPolicy = nativeOpenPolicy ?? (managedWorkspace ? "managed" : "editor");
  const [document, setDocument] = useState<DocumentState>(() => createInitialDocumentState());
  const [tabs, setTabs] = useState<MarkdownDocumentTab[]>(() => [createDocumentTab(createInitialDocumentState(), "untitled:0")]);
  const [activeTabId, setActiveTabId] = useState<string | null>("untitled:0");
  const [workspaceSurface, setWorkspaceSurface] = useState<WorkspaceSurface>("restoring");
  const [nativeOpenedPathsReady, setNativeOpenedPathsReady] = useState(false);
  const [recentFiles, setRecentFiles] = useState<RecentMarkdownFile[]>([]);
  const [deferredMarkdownSummary, setDeferredMarkdownSummary] = useState<MarkdownDocumentSummary | null>(null);
  const documentRef = useRef(document);
  const tabsRef = useRef(tabs);
  const activeTabIdRef = useRef<string | null>(activeTabId);
  const workspaceReadyRef = useRef(workspaceReady);
  workspaceReadyRef.current = workspaceReady;
  const untitledTabIndexRef = useRef(1);
  const openedFromNativeRef = useRef(false);
  const nativeOpenedPathsInitializationRef = useRef<Promise<boolean> | null>(null);
  const externalFileStartupPathRef = useRef<string | null>(null);
  const externalFolderStartupPathRef = useRef<string | null>(null);
  const startupWorkspaceRestoreKeyRef = useRef<string | null>(null);
  const nativeWindowCloseScheduledRef = useRef(false);
  const nativeCloseBlockedRef = useRef(nativeCloseBlocked);
  nativeCloseBlockedRef.current = nativeCloseBlocked;
  const dirtyFileSavePromiseRef = useRef<Promise<SavedNativeMarkdownFile[]> | null>(null);
  const dirtyFileWriteTailRef = useRef<Promise<unknown>>(Promise.resolve());
  const editorSyncStateRef = useRef<EditorSyncState | null>(null);
  if (editorSyncStateRef.current === null) editorSyncStateRef.current = createEditorSyncState();
  const editorSyncState = editorSyncStateRef.current;
  const persistWorkspaceState = useCallback((patch: Partial<StoredWorkspaceState>) => {
    if (workspacePersistencePolicy === "isolated") return Promise.resolve();
    return persistDesktopWorkspaceState(managedWorkspace ? managedWorkspaceStatePatch(patch) : patch);
  }, [managedWorkspace, workspacePersistencePolicy]);
  const largeDocumentSummariesBlocked = useMemo(
    () => shouldBlockLargeMarkdownVisual(document.content, { sizeBytes: document.sizeBytes }),
    [document.content, document.sizeBytes]
  );
  const markdownSummaryDeferred = useMemo(
    () =>
      !largeDocumentSummariesBlocked &&
      shouldDeferMarkdownSummary(document.content, { sizeBytes: document.sizeBytes }),
    [document.content, document.sizeBytes, largeDocumentSummariesBlocked]
  );
  const markdownSummaryKey = useMemo(() => markdownDocumentSummaryKey(document), [
    document.content.length,
    document.revision,
    document.sizeBytes
  ]);
  const immediateMarkdownSummary = useMemo(
    () =>
      largeDocumentSummariesBlocked || markdownSummaryDeferred
        ? emptyMarkdownDocumentSummary
        : calculateMarkdownDocumentSummary(document.content, {
          chars: document.content.length,
          deferred: false,
          name: document.name,
          path: document.path,
          sizeBytes: document.sizeBytes ?? null
        }),
    [
      document.content,
      document.name,
      document.path,
      document.sizeBytes,
      largeDocumentSummariesBlocked,
      markdownSummaryDeferred
    ]
  );
  const currentDeferredMarkdownSummary =
    deferredMarkdownSummary?.key === markdownSummaryKey ? deferredMarkdownSummary : null;
  const outlineItems = markdownSummaryDeferred
    ? currentDeferredMarkdownSummary?.outlineItems ?? emptyMarkdownDocumentSummary.outlineItems
    : immediateMarkdownSummary.outlineItems;
  const wordCount = markdownSummaryDeferred
    ? currentDeferredMarkdownSummary?.wordCount ?? emptyMarkdownDocumentSummary.wordCount
    : immediateMarkdownSummary.wordCount;
  const watchedMarkdownFilePathsKey = useMemo(() => openFilePathsFromTabs(tabs).join("\n"), [tabs]);

  useEffect(() => {
    if (!markdownSummaryDeferred) return;

    let active = true;
    const summaryContent = document.content;
    const summaryKey = markdownSummaryKey;
    const cancel = scheduleMarkdownSummaryIdle(() => {
      if (!active) return;

      const summary = calculateMarkdownDocumentSummary(summaryContent, {
        chars: summaryContent.length,
        deferred: true,
        name: document.name,
        path: document.path,
        sizeBytes: document.sizeBytes ?? null
      });
      if (!active) return;

      setDeferredMarkdownSummary({
        key: summaryKey,
        ...summary
      });
    });

    return () => {
      active = false;
      cancel();
    };
  }, [
    document.content,
    document.name,
    document.path,
    document.sizeBytes,
    markdownSummaryDeferred,
    markdownSummaryKey
  ]);

  useEffect(() => {
    documentRef.current = document;
  }, [document]);

  useEffect(() => {
    tabsRef.current = tabs;
  }, [tabs]);

  useEffect(() => {
    activeTabIdRef.current = activeTabId;
  }, [activeTabId]);

  useEffect(() => {
    if (!workspaceReady) startupWorkspaceRestoreKeyRef.current = null;
    setWorkspaceSurface((currentSurface) => {
      if (!workspaceReady) return "recovery";
      return currentSurface === "recovery" ? "restoring" : currentSurface;
    });
  }, [workspaceReady]);

  const rememberRecentMarkdownFile = useCallback((file: RecentMarkdownFile) => {
    if (managedWorkspace) return;

    const path = file.path.trim();
    if (!path) return;

    const recentFile = {
      name: file.name.trim() || pathNameFromPath(path),
      path
    };
    setRecentFiles((current) => prependRecentMarkdownFile(current, recentFile));
    saveStoredRecentMarkdownFile(recentFile).catch(() => {});
  }, [managedWorkspace]);

  const forgetRecentMarkdownFile = useCallback((path: string) => {
    if (managedWorkspace) return;

    const normalizedPath = path.trim();
    if (!normalizedPath) return;

    setRecentFiles((current) => current.filter((file) => file.path !== normalizedPath));
    removeStoredRecentMarkdownFile(normalizedPath).catch(() => {});
  }, [managedWorkspace]);

  const clearRecentMarkdownFiles = useCallback(() => {
    setRecentFiles([]);
    if (managedWorkspace) return;
    clearStoredRecentMarkdownFiles().catch(() => {});
  }, [managedWorkspace]);

  const currentMarkdown = useCallback(() => {
    const current = documentRef.current;
    if (!current.open) return current.content;
    if (!resolveEditorReady(editorReady)) return current.content;

    return getCurrentMarkdown(current.content);
  }, [editorReady, getCurrentMarkdown]);

  const isActiveEditorMarkdownEquivalent = useCallback((markdown: string) => {
    if (!resolveEditorReady(editorReady)) return undefined;

    return isCurrentMarkdownEquivalent?.(markdown);
  }, [editorReady, isCurrentMarkdownEquivalent]);

  const registerWindowRestoreState = useCallback((filePath: string | null, openFilePaths: string[]) => {
    if (managedWorkspace || workspacePersistencePolicy === "isolated") return;
    setNativeEditorWindowRestoreState({ filePath, openFilePaths }).catch(() => {});
  }, [managedWorkspace, workspacePersistencePolicy]);

  const handoffPrimarySavedCopy = useCallback(async (
    savedFile: SavedNativeMarkdownFile,
    selectedNewTarget: boolean
  ) => {
    if (!selectedNewTarget || saveAsWorkspacePolicy.kind !== "primary") return false;
    if (
      saveAsWorkspacePolicy.root &&
      documentPathIsWithinWorkspaceRoot(saveAsWorkspacePolicy.root, savedFile.path)
    ) {
      return false;
    }

    await openNativeMarkdownFileInNewWindow(savedFile.path);
    return true;
  }, [saveAsWorkspacePolicy]);

  const shouldRetargetSavedDocument = useCallback((selectedNewTarget: boolean) => {
    return selectedNewTarget && saveAsWorkspacePolicy.kind === "standalone";
  }, [saveAsWorkspacePolicy.kind]);

  const setActiveDocument = useCallback((nextDocument: DocumentState) => {
    documentRef.current = nextDocument;
    setDocument(nextDocument);
    setWorkspaceSurface(nextDocument.open
      ? "editor"
      : workspaceReadyRef.current ? "home" : "recovery");

    const fallbackActiveTabId = nextDocument.open
      ? nextDocument.path ? fileTabId(nextDocument.path) : "untitled:0"
      : null;
    const currentActiveTabId = activeTabIdRef.current ?? fallbackActiveTabId;
    if (!currentActiveTabId) return;

    activeTabIdRef.current = currentActiveTabId;
    setActiveTabId(currentActiveTabId);

    const nextTab = createDocumentTab(nextDocument, currentActiveTabId);
    const currentTabs = tabsRef.current;
    const tabExists = currentTabs.some((tab) => tab.id === currentActiveTabId);
    const nextTabs = tabExists
      ? currentTabs.map((tab) => tab.id === currentActiveTabId ? nextTab : tab)
      : [...currentTabs, nextTab];
    tabsRef.current = nextTabs;
    setTabs(nextTabs);
  }, []);

  const setActiveTabState = useCallback((nextTabs: MarkdownDocumentTab[], nextActiveTabId: string | null) => {
    const activeTab = nextTabs.find((tab) => tab.id === nextActiveTabId) ?? null;
    const nextDocument = activeTab
      ? documentFromTab(activeTab)
      : {
        path: null,
        name: "",
        content: "",
        deleted: false,
        dirty: false,
        open: false,
        revision: documentRef.current.revision + 1
      };

    tabsRef.current = nextTabs;
    activeTabIdRef.current = nextActiveTabId;
    documentRef.current = nextDocument;
    setTabs(nextTabs);
    setActiveTabId(nextActiveTabId);
    setDocument(nextDocument);
    setWorkspaceSurface(activeTab
      ? "editor"
      : workspaceReadyRef.current ? "home" : "recovery");
  }, []);

  const createUntitledTabId = useCallback(() => {
    const tabId = `untitled:${untitledTabIndexRef.current}`;
    untitledTabIndexRef.current += 1;
    return tabId;
  }, []);

  const syncActiveDocumentFromEditor = useCallback(() => {
    const current = documentRef.current;
    if (!current.open) return current;

    const content = currentMarkdown();
    if (current.content === content) return current;
    const currentActiveTabId = activeTabIdRef.current;
    if (
      !current.dirty &&
      editorSyncState.isSavedVisualEditorStaleContent(currentActiveTabId, current.content, content)
    ) {
      return current;
    }
    if (!current.dirty && editorSyncState.isCleanVisualMarkdownBaseline(currentActiveTabId, content)) return current;

    const editorContentEquivalent = isActiveEditorMarkdownEquivalent(current.content);
    const nextDocument =
      !current.dirty && (editorContentEquivalent === true || isEquivalentEditorMarkdown(current.content, content))
        ? { ...current, content, dirty: false }
        : { ...current, content, dirty: true };
    if (!current.dirty && nextDocument.dirty) editorSyncState.clearCleanVisualMarkdownBaseline(currentActiveTabId);

    const nextTabs = currentActiveTabId
      ? tabsRef.current.map((tab) => tab.id === currentActiveTabId ? createDocumentTab(nextDocument, tab.id) : tab)
      : tabsRef.current;
    setActiveDocument(nextDocument);
    persistWorkspaceState(draftWorkspacePatchFromTabs(nextTabs, currentActiveTabId));
    return nextDocument;
  }, [currentMarkdown, editorSyncState, isActiveEditorMarkdownEquivalent, setActiveDocument]);

  const syncActiveDocumentDraftSnapshot = useCallback(() => {
    const syncedDocument = syncActiveDocumentFromEditor();
    const currentActiveTabId = activeTabIdRef.current;
    const syncedTabs = currentActiveTabId
      ? tabsRef.current.map((tab) => tab.id === currentActiveTabId ? createDocumentTab(syncedDocument, tab.id) : tab)
      : tabsRef.current;
    tabsRef.current = syncedTabs;

    return {
      activeTabId: currentActiveTabId,
      tabs: syncedTabs
    };
  }, [syncActiveDocumentFromEditor]);

  const persistActiveDocumentDraftSnapshot = useCallback(() => {
    const snapshot = syncActiveDocumentDraftSnapshot();
    return persistWorkspaceState(draftWorkspacePatchFromTabs(snapshot.tabs, snapshot.activeTabId));
  }, [syncActiveDocumentDraftSnapshot]);

  const getDirtyMarkdownFileContent = useCallback((path: string) => {
    const snapshot = syncActiveDocumentDraftSnapshot();
    const tab = snapshot.tabs.find((candidate) => sameNativePath(candidate.path, path));

    return tab?.dirty ? tab.content : null;
  }, [syncActiveDocumentDraftSnapshot]);

  const persistNativeEditorWindowRestoreSnapshot = useCallback(async () => {
    if (managedWorkspace) return;

    try {
      // The native window list only lives in the current process. Copy it into settings
      // before an app-driven exit so secondary editor windows survive restart.
      const openWindows = await listNativeEditorWindowRestoreStates();
      await persistWorkspaceState({ openWindows });
    } catch {
      // Snapshot persistence is best-effort; the current window state is still saved independently.
    }
  }, [managedWorkspace]);

  const hasDiscardableUnsavedChanges = useCallback(() => {
    const current = documentRef.current;
    if (!current.open) return false;
    if (current.path === null && current.content.trim().length === 0) return false;

    if (!current.dirty) {
      const editorContentEquivalent = isActiveEditorMarkdownEquivalent(current.content);
      if (editorContentEquivalent) return false;

      const editorMarkdown = currentMarkdown();
      if (editorSyncState.isSavedVisualEditorStaleContent(activeTabIdRef.current, current.content, editorMarkdown)) return false;
      if (editorSyncState.isCleanVisualMarkdownBaseline(activeTabIdRef.current, editorMarkdown)) return false;
      if (isEquivalentEditorMarkdown(editorMarkdown, current.content)) return false;
      if (editorContentEquivalent === false && current.path !== null) return true;

      return current.path !== null || editorMarkdown.trim().length > 0;
    }
    if (current.path) return true;

    const editorMarkdown = currentMarkdown();
    return editorMarkdown.trim().length > 0;
  }, [currentMarkdown, editorSyncState, isActiveEditorMarkdownEquivalent]);

  const hasDiscardableTabChanges = useCallback((tab: MarkdownDocumentTab) => {
    if (tab.id === activeTabIdRef.current) return hasDiscardableUnsavedChanges();
    if (!tab.open) return false;
    if (tab.path === null && tab.content.trim().length === 0) return false;
    if (tab.dirty) return tab.path !== null || tab.content.trim().length > 0;

    return false;
  }, [hasDiscardableUnsavedChanges]);

  const currentMarkdownForSave = useCallback((current: DocumentState, tabId: string | null | undefined) => {
    const content = currentMarkdown();
    if (!current.dirty && editorSyncState.isSavedVisualEditorStaleContent(tabId, current.content, content)) {
      return current.content;
    }
    if (!current.dirty && editorSyncState.isCleanVisualMarkdownBaseline(tabId, content)) {
      return current.content;
    }

    return content;
  }, [currentMarkdown, editorSyncState]);

  const confirmCanDiscardCurrentDocument = useCallback(() => {
    const dirtyTab = tabsRef.current.find((tab) => hasDiscardableTabChanges(tab));
    if (!dirtyTab) return true;

    return confirmDiscardUnsavedChanges?.(documentFromTab(dirtyTab)) ?? true;
  }, [
    confirmDiscardUnsavedChanges,
    hasDiscardableTabChanges
  ]);

  const requestAppExit = useCallback(async () => {
    if (nativeCloseBlockedRef.current) return;

    const draftPersistence = persistActiveDocumentDraftSnapshot();
    const canDiscard = await confirmCanDiscardCurrentDocument();
    if (!canDiscard) return;

    await draftPersistence;
    await persistNativeEditorWindowRestoreSnapshot();
    await exitNativeApp();
  }, [confirmCanDiscardCurrentDocument, persistActiveDocumentDraftSnapshot, persistNativeEditorWindowRestoreSnapshot]);

  const handleMarkdownChange = useCallback((content: string, options: MarkdownChangeOptions = {}) => {
    if (!resolveEditorReady(editorReady)) return;

    const current = documentRef.current;
    if (options.documentRevision !== undefined && options.documentRevision !== current.revision) return;
    if (!current.open || current.content === content) return;
    const currentActiveTabId = activeTabIdRef.current;
    if (
      !current.dirty &&
      options.surface === "visual" &&
      editorSyncState.isCleanVisualMarkdownBaseline(currentActiveTabId, content)
    ) {
      return;
    }
    const canKeepEquivalentMarkdownClean = options.surface !== "source";
    const editorContentEquivalent = isActiveEditorMarkdownEquivalent(current.content);
    if (
      !current.dirty &&
      options.surface === "visual" &&
      editorContentEquivalent === true &&
      !isEquivalentEditorMarkdown(current.content, content) &&
      isActiveEditorMarkdownEquivalent(content) === false
    ) {
      return;
    }
    if (!current.dirty) {
      editorSyncState.rememberCleanVisualContentBeforeDirty(currentActiveTabId, current.content, content, options.surface);
    }
    const nextDocument =
      !current.dirty &&
      canKeepEquivalentMarkdownClean &&
      (editorContentEquivalent === true || isEquivalentEditorMarkdown(current.content, content))
        ? { ...current, content, dirty: false }
        : { ...current, content, dirty: true };
    if (!current.dirty && nextDocument.dirty) editorSyncState.clearCleanVisualMarkdownBaseline(currentActiveTabId);

    const nextTabs = currentActiveTabId
      ? tabsRef.current.map((tab) => tab.id === currentActiveTabId ? createDocumentTab(nextDocument, tab.id) : tab)
      : tabsRef.current;
    setActiveDocument(nextDocument);
    persistWorkspaceState(draftWorkspacePatchFromTabs(nextTabs, currentActiveTabId));
  }, [
    editorSyncState,
    editorReady,
    isActiveEditorMarkdownEquivalent,
    setActiveDocument
  ]);

  const handleMarkdownTabChange = useCallback((tabId: string, content: string, options: MarkdownChangeOptions = {}) => {
    if (tabId === activeTabIdRef.current) {
      handleMarkdownChange(content, options);
      return;
    }

    setTabs((currentTabs) => {
      const nextTabs = currentTabs.map((tab) => {
        if (tab.id !== tabId || !tab.open || tab.content === content) return tab;
        if (options.documentRevision !== undefined && options.documentRevision !== tab.revision) return tab;
        if (
          !tab.dirty &&
          options.surface === "visual" &&
          editorSyncState.isCleanVisualMarkdownBaseline(tab.id, content)
        ) {
          return tab;
        }
        if (
          !tab.dirty &&
          options.surface === "visual" &&
          options.documentRevision !== undefined &&
          !isEquivalentEditorMarkdown(tab.content, content)
        ) {
          return tab;
        }
        if (!tab.dirty) editorSyncState.rememberCleanVisualContentBeforeDirty(tab.id, tab.content, content, options.surface);
        const canKeepEquivalentMarkdownClean = options.surface !== "source";
        const contentEquivalent = canKeepEquivalentMarkdownClean && isEquivalentEditorMarkdown(tab.content, content);
        if (!tab.dirty && !contentEquivalent) {
          editorSyncState.clearCleanVisualMarkdownBaseline(tab.id);
        }

        return {
          ...tab,
          content,
          dirty: tab.dirty || !contentEquivalent
        };
      });

      tabsRef.current = nextTabs;
      persistWorkspaceState(draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current));
      return nextTabs;
    });
  }, [editorSyncState, handleMarkdownChange]);

  const rememberMarkdownTabVisualBaseline = useCallback((tabId: string, content: string) => {
    const tab = tabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab || !tab.open || tab.dirty) return false;

    editorSyncState.rememberCleanVisualMarkdownBaseline(tabId, content);
    return true;
  }, [editorSyncState]);

  const restoreDocumentContent = useCallback((content: string) => {
    const current = documentRef.current;
    if (!current.open) {
      debug(() => ["[markra-history] document restore ignored", {
        reason: "document closed"
      }]);
      return false;
    }

    const nextDocument = {
      ...current,
      content,
      dirty: true,
      revision: current.revision + 1
    };
    const currentActiveTabId = activeTabIdRef.current;
    const nextTabs = currentActiveTabId
      ? tabsRef.current.map((tab) => tab.id === currentActiveTabId ? createDocumentTab(nextDocument, tab.id) : tab)
      : tabsRef.current;

    setActiveDocument(nextDocument);
    debug(() => ["[markra-history] document restore state updated", {
      activeTabId: currentActiveTabId,
      contentsChars: content.length,
      dirty: nextDocument.dirty,
      nextRevision: nextDocument.revision,
      path: nextDocument.path,
      previousRevision: current.revision
    }]);
    persistWorkspaceState(draftWorkspacePatchFromTabs(nextTabs, currentActiveTabId));
    return true;
  }, [setActiveDocument]);

  const resetToBlankDocument = useCallback((
    options: CreateBlankDocumentOptions = {},
    savedFile: SavedNativeMarkdownFile | null = null
  ) => {
    const content = options.content ?? "";
    const nextDocument = {
      path: savedFile?.path ?? null,
      name: savedFile?.name || blankDocumentName(options.name),
      content,
      deleted: false,
      dirty: savedFile === null,
      open: true,
      revision: documentRef.current.revision + 1
    };

    if (documentTabsEnabled) {
      syncActiveDocumentFromEditor();
      const tab = createDocumentTab(
        nextDocument,
        savedFile ? fileTabId(savedFile.path) : createUntitledTabId()
      );
      const nextTabs = [...tabsRef.current, tab];
      setActiveTabState(nextTabs, tab.id);
      const nextActiveFilePath = activeFilePathFromTabs(nextTabs, tab.id);
      const nextOpenFilePaths = openFilePathsFromTabs(nextTabs);
      if (savedFile) editorSyncState.rememberSavedVisualEditorStaleContent(tab.id, content);
      registerWindowRestoreState(nextActiveFilePath, nextOpenFilePaths);
      persistWorkspaceState({
        ...draftWorkspacePatchFromTabs(nextTabs, tab.id),
        filePath: nextActiveFilePath,
        openFilePaths: nextOpenFilePaths
      });
    } else {
      setActiveDocument(nextDocument);
      const nextOpenFilePaths = savedFile ? [savedFile.path] : [];
      registerWindowRestoreState(savedFile?.path ?? null, nextOpenFilePaths);
      const nextTabs = [createDocumentTab(nextDocument, activeTabIdRef.current ?? "untitled:0")];
      persistWorkspaceState({
        ...draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current),
        filePath: savedFile?.path ?? null,
        openFilePaths: nextOpenFilePaths
      });
    }
    if (savedFile) rememberRecentMarkdownFile(savedFile);
    return true;
  }, [
    createUntitledTabId,
    documentTabsEnabled,
    editorSyncState,
    registerWindowRestoreState,
    rememberRecentMarkdownFile,
    setActiveDocument,
    setActiveTabState,
    syncActiveDocumentFromEditor
  ]);

  const createBlankDocument = useCallback(async (options: CreateBlankDocumentOptions = {}) => {
    const create = async () => {
      if (!isKernelWorkspaceDirectory(defaultSaveDirectory)) {
        return resetToBlankDocument(options);
      }

      const savedFile = await saveNativeMarkdownFile({
        ...defaultSaveDirectoryInput(defaultSaveDirectory, null),
        contents: options.content ?? "",
        path: null,
        suggestedName: blankDocumentName(options.name)
      });
      if (!savedFile) return false;

      return resetToBlankDocument(options, savedFile);
    };

    if (documentTabsEnabled) return create();

    const canDiscard = confirmCanDiscardCurrentDocument();
    if (typeof canDiscard === "boolean") {
      if (!canDiscard) return false;

      return create();
    }

    const confirmed = await canDiscard;
    if (!confirmed) return false;

    return create();
  }, [confirmCanDiscardCurrentDocument, defaultSaveDirectory, documentTabsEnabled, resetToBlankDocument]);

  const clearOpenDocument = useCallback((options: ClearOpenDocumentOptions = {}) => {
    const openBlank = options.openBlank === true;
    const nextDocument = {
      path: null,
      name: openBlank ? "Untitled.md" : "",
      content: "",
      deleted: false,
      dirty: false,
      open: openBlank,
      revision: documentRef.current.revision + 1
    };
    const nextActiveTabId = openBlank ? "untitled:0" : null;
    const nextTabs = openBlank ? [createDocumentTab(nextDocument, nextActiveTabId!)] : [];
    tabsRef.current = nextTabs;
    activeTabIdRef.current = nextActiveTabId;
    editorSyncState.clearAll();
    setTabs(nextTabs);
    setActiveTabId(nextActiveTabId);
    setDocument(nextDocument);
    documentRef.current = nextDocument;
    setWorkspaceSurface(openBlank
      ? "editor"
      : workspaceReadyRef.current ? "home" : "recovery");
    registerWindowRestoreState(null, []);
    if (options.persistWorkspace !== false) {
      persistWorkspaceState({
        activeDraftId: null,
        draftTabs: openBlank ? draftWorkspacePatchFromTabs(nextTabs, nextActiveTabId).draftTabs : [],
        filePath: null,
        openFilePaths: []
      });
    }
  }, [editorSyncState, persistWorkspaceState, registerWindowRestoreState]);

  const readMarkdownFileWithPerformance = useCallback(
    (path: string, reason: string) =>
      measureAppPerformanceAsync("markdown-file-read", () => readNativeMarkdownFile(path), {
        path,
        reason
      }),
    []
  );

  const applyDiskFileToCleanOpenTab = useCallback((
    file: NativeMarkdownFile,
    reason: string,
    expectedTab?: { content: string; id: string; revision: number }
  ) => {
    const currentTabs = tabsRef.current;
    const targetTab = currentTabs.find((tab) => tab.path !== null && sameNativePath(tab.path, file.path));
    // Tab-selection refreshes are async; ignore disk reads if the tab changed while the read was in flight.
    const staleRequest =
      expectedTab &&
      (
        targetTab?.id !== expectedTab.id ||
        targetTab.revision !== expectedTab.revision ||
        targetTab.content !== expectedTab.content
      );
    if (!targetTab || staleRequest || targetTab.dirty || (!targetTab.deleted && targetTab.content === file.content)) {
      debug(() => ["[markra-history] disk file event ignored", {
        diskPath: file.path,
        reason: !targetTab
          ? "tab missing"
          : staleRequest
            ? "stale request"
            : targetTab.dirty
              ? "tab dirty"
              : !targetTab.deleted && targetTab.content === file.content
                ? "contents unchanged"
                : "unknown",
        source: reason,
        tabId: targetTab?.id ?? null
      }]);
      return false;
    }

    const activeTabChanged = targetTab.id === activeTabIdRef.current;
    const activeEditorUpdated =
      activeTabChanged &&
      onActiveDiskFileContentChange?.({
        content: file.content,
        path: file.path,
        previousContent: targetTab.content,
        revision: targetTab.revision,
        source: reason
      }) === true;

    // Keep the active editor mounted when it already accepted the disk update as an undoable transaction.
    const nextDocument = {
      path: file.path,
      name: file.name,
      content: file.content,
      sizeBytes: file.sizeBytes,
      deleted: false,
      dirty: false,
      open: true,
      revision: activeEditorUpdated ? targetTab.revision : targetTab.revision + 1
    };
    const nextTabs = currentTabs.map((tab) =>
      tab.id === targetTab.id ? createDocumentTab(nextDocument, tab.id) : tab
    );

    // A disk reload moves the clean reference forward; an old visual baseline would hide
    // undoing back to the previous disk content as a real unsaved edit.
    editorSyncState.clearCleanVisualMarkdownBaseline(targetTab.id);
    tabsRef.current = nextTabs;
    setTabs(nextTabs);
    if (activeTabChanged) {
      documentRef.current = nextDocument;
      setDocument(nextDocument);
    }

    debug(() => ["[markra-history] disk file content applied", {
      contentsChars: file.content.length,
      diskPath: file.path,
      nextRevision: nextDocument.revision,
      source: reason,
      tabId: targetTab.id
    }]);
    return true;
  }, [editorSyncState, onActiveDiskFileContentChange]);

  const refreshCleanOpenTabFromDisk = useCallback((path: string, reason: string) => {
    const currentTab = tabsRef.current.find((tab) => tab.path !== null && sameNativePath(tab.path, path));
    if (!currentTab || currentTab.dirty) return;

    const expectedTab = {
      content: currentTab.content,
      id: currentTab.id,
      revision: currentTab.revision
    };

    readMarkdownFileWithPerformance(path, reason)
      .then((file) => {
        applyDiskFileToCleanOpenTab(file, reason, expectedTab);
      })
      .catch(() => {});
  }, [applyDiskFileToCleanOpenTab, readMarkdownFileWithPerformance]);

  const applyNativeMarkdownFile = useCallback(
    (file: NativeMarkdownFile, updateTreeRoot = true, managed = false) => {
      return measureAppPerformance("markdown-document-apply", () => {
        const nextDocument = {
          path: file.path,
          name: file.name,
          content: file.content,
          sizeBytes: file.sizeBytes,
          deleted: false,
          dirty: false,
          open: true,
          revision: documentRef.current.revision + 1
        };

        let nextOpenFilePaths = [file.path];

        if (documentTabsEnabled) {
          syncActiveDocumentFromEditor();
          const currentTabs = tabsRef.current;
          const existingTab = currentTabs.find((tab) => sameNativePath(tab.path, file.path));
          let nextTabs: MarkdownDocumentTab[];
          let nextActiveTabId: string;

          if (existingTab) {
            nextTabs = currentTabs;
            nextActiveTabId = existingTab.id;
          } else {
            const activeTabIsEmptyUntitled = currentTabs.some((tab) =>
              tab.id === activeTabIdRef.current && isEmptyUntitledDocument(documentFromTab(tab))
            );
            const nextTabId = activeTabIsEmptyUntitled && activeTabIdRef.current ? activeTabIdRef.current : fileTabId(file.path);
            const nextTab = createDocumentTab(nextDocument, nextTabId);
            nextTabs = activeTabIsEmptyUntitled
              ? currentTabs.map((tab) => tab.id === activeTabIdRef.current ? nextTab : tab)
              : [...currentTabs, nextTab];
            nextActiveTabId = nextTab.id;
          }

          setActiveTabState(nextTabs, nextActiveTabId);
          nextOpenFilePaths = openFilePathsFromTabs(nextTabs);
        } else {
          setActiveDocument(nextDocument);
        }

        if (updateTreeRoot) onTreeRootFromFilePath(file.path);
        if (managedWorkspace) {
          const nextDraftTabs = documentTabsEnabled
            ? tabsRef.current
            : [createDocumentTab(nextDocument, activeTabIdRef.current ?? fileTabId(file.path))];
          persistWorkspaceState({
            ...draftWorkspacePatchFromTabs(nextDraftTabs, activeTabIdRef.current),
            filePath: file.path,
            openFilePaths: nextOpenFilePaths
          });
        } else if (!managed) {
          rememberRecentMarkdownFile({ name: file.name, path: file.path });
          registerWindowRestoreState(file.path, nextOpenFilePaths);
          const nextDraftTabs = documentTabsEnabled
            ? tabsRef.current
            : [createDocumentTab(nextDocument, activeTabIdRef.current ?? fileTabId(file.path))];
          persistWorkspaceState({
            ...draftWorkspacePatchFromTabs(nextDraftTabs, activeTabIdRef.current),
            filePath: file.path,
            openFilePaths: nextOpenFilePaths,
            ...(updateTreeRoot ? { folderName: null, folderPath: null } : {})
          });
        }
      }, {
        name: file.name,
        path: file.path,
        sizeBytes: file.sizeBytes ?? null,
        updateTreeRoot
      });
    },
    [documentTabsEnabled, managedWorkspace, onTreeRootFromFilePath, persistWorkspaceState, registerWindowRestoreState, rememberRecentMarkdownFile, setActiveDocument, setActiveTabState, syncActiveDocumentFromEditor]
  );

  const loadNativeMarkdownPath = useCallback(
    async (path: string, updateTreeRoot = true, managed = false) => {
      const file = await readMarkdownFileWithPerformance(path, "load-path");
      applyNativeMarkdownFile(file, updateTreeRoot, managed);
    },
    [applyNativeMarkdownFile, readMarkdownFileWithPerformance]
  );

  const openRecentMarkdownFile = useCallback(
    async (file: RecentMarkdownFile) => {
      const path = file.path.trim();
      if (!path) return false;

      try {
        if (!documentTabsEnabled) {
          const canDiscard = await confirmCanDiscardCurrentDocument();
          if (!canDiscard) return false;
        }

        await loadNativeMarkdownPath(path);
        return true;
      } catch {
        forgetRecentMarkdownFile(path);
        return false;
      }
    },
    [confirmCanDiscardCurrentDocument, documentTabsEnabled, forgetRecentMarkdownFile, loadNativeMarkdownPath]
  );

  const restoreNativeMarkdownFiles = useCallback(
    async (
      paths: string[],
      activeFilePath: string | null,
      updateTreeRoot = true,
      shouldApply: () => boolean = () => true
    ) => {
      const files: NativeMarkdownFile[] = [];
      const failedPaths: string[] = [];

      for (const path of paths) {
        try {
          const file = await readMarkdownFileWithPerformance(path, "restore-workspace");
          if (!shouldApply()) {
            return {
              activeFilePath: null,
              cancelled: true,
              failedPaths,
              files,
              survivingPaths: files.map((candidate) => candidate.path)
            } satisfies NativeMarkdownFilesRestoreResult;
          }
          files.push(file);
        } catch {
          // Missing or moved files should not block restoring the rest of the workspace.
          if (!shouldApply()) {
            return {
              activeFilePath: null,
              cancelled: true,
              failedPaths,
              files,
              survivingPaths: files.map((candidate) => candidate.path)
            } satisfies NativeMarkdownFilesRestoreResult;
          }
          failedPaths.push(path);
        }
      }

      if (!shouldApply()) {
        return {
          activeFilePath: null,
          cancelled: true,
          failedPaths,
          files,
          survivingPaths: files.map((candidate) => candidate.path)
        } satisfies NativeMarkdownFilesRestoreResult;
      }

      const activeFile =
        files.find((file) => file.path === activeFilePath) ??
        files.at(-1) ??
        null;

      if (documentTabsEnabled && files.length > 0) {
        const nextTabs = files.map((file) =>
          createDocumentTab({
            path: file.path,
            name: file.name,
            content: file.content,
            sizeBytes: file.sizeBytes,
            deleted: false,
            dirty: false,
            open: true,
            revision: documentRef.current.revision + 1
          }, fileTabId(file.path))
        );

        setActiveTabState(nextTabs, activeFile ? fileTabId(activeFile.path) : nextTabs[0]?.id ?? null);
      } else if (activeFile) {
        setActiveDocument({
          path: activeFile.path,
          name: activeFile.name,
          content: activeFile.content,
          sizeBytes: activeFile.sizeBytes,
          deleted: false,
          dirty: false,
          open: true,
          revision: documentRef.current.revision + 1
        });
      }

      if (updateTreeRoot && activeFile) onTreeRootFromFilePath(activeFile.path);
      registerWindowRestoreState(activeFile?.path ?? null, files.map((file) => file.path));
      return {
        activeFilePath: activeFile?.path ?? null,
        cancelled: false,
        failedPaths,
        files,
        survivingPaths: files.map((file) => file.path)
      } satisfies NativeMarkdownFilesRestoreResult;
    },
    [documentTabsEnabled, onTreeRootFromFilePath, readMarkdownFileWithPerformance, registerWindowRestoreState, setActiveDocument, setActiveTabState]
  );

  const restoreWorkspaceDraftTabs = useCallback((
    draftTabs: readonly StoredWorkspaceDraftTab[] | undefined,
    activeDraftId: string | null | undefined
  ) => {
    if (!draftTabs?.length) {
      return {
        activeFilePath: activeFilePathFromTabs(tabsRef.current, activeTabIdRef.current),
        activeTabId: activeTabIdRef.current,
        recoverableDraftCount: 0,
        tabs: tabsRef.current
      } satisfies WorkspaceDraftRestoreResult;
    }

    const draftDocumentTabs = draftTabs.map((draft, index) =>
      createDocumentTab(documentFromDraftTab(draft, documentRef.current.revision + index + 1), draft.id)
    );
    const currentTabs = tabsRef.current.filter((tab) =>
      !isPristineUntitledDocument(documentFromTab(tab)) &&
      !draftDocumentTabs.some((draftTab) => draftTab.id === tab.id || (draftTab.path !== null && draftTab.path === tab.path))
    );
    const nextTabs = [...currentTabs, ...draftDocumentTabs];
    const nextActiveTabId =
      activeDraftId && nextTabs.some((tab) => tab.id === activeDraftId)
        ? activeDraftId
        : activeTabIdRef.current && nextTabs.some((tab) => tab.id === activeTabIdRef.current)
          ? activeTabIdRef.current
          : draftDocumentTabs.at(-1)?.id ?? nextTabs.at(-1)?.id ?? null;
    const nextActiveFilePath = activeFilePathFromTabs(nextTabs, nextActiveTabId);
    setActiveTabState(nextTabs, nextActiveTabId);
    registerWindowRestoreState(nextActiveFilePath, openFilePathsFromTabs(nextTabs));

    if (nextActiveFilePath) onTreeRootFromFilePath(nextActiveFilePath);
    return {
      activeFilePath: nextActiveFilePath,
      activeTabId: nextActiveTabId,
      recoverableDraftCount: draftDocumentTabs.length,
      tabs: nextTabs
    } satisfies WorkspaceDraftRestoreResult;
  }, [onTreeRootFromFilePath, registerWindowRestoreState, setActiveTabState]);

  const openMarkdownFile = useCallback(async (options: OpenMarkdownFileOptions = {}) => {
    const file = await openNativeMarkdownFile(
      options.pickerTitle ? { title: options.pickerTitle } : undefined
    );
    if (!file) return;

    if (!documentTabsEnabled) {
      const canDiscard = await confirmCanDiscardCurrentDocument();
      if (!canDiscard) return;
    }

    applyNativeMarkdownFile(file, windowContext.kind === "primary");
  }, [applyNativeMarkdownFile, confirmCanDiscardCurrentDocument, documentTabsEnabled, windowContext.kind]);

  const openTreeMarkdownFile = useCallback(
    async (file: NativeMarkdownFolderFile, options: OpenTreeMarkdownFileOptions = {}) => {
      try {
        if (!documentTabsEnabled) {
          const canDiscard = await confirmCanDiscardCurrentDocument();
          if (!canDiscard) return false;
        }

        await loadNativeMarkdownPath(file.path, false, options.managed === true);
        return true;
      } catch {
        // Missing or moved files should leave the tree available for another choice.
        return false;
      }
    },
    [confirmCanDiscardCurrentDocument, documentTabsEnabled, loadNativeMarkdownPath]
  );

  const openTreeMarkdownFileInBackground = useCallback(
    async (file: NativeMarkdownFolderFile) => {
      try {
        const nativeFile = await readMarkdownFileWithPerformance(file.path, "background-tab");
        const nextDocument = {
          path: nativeFile.path,
          name: nativeFile.name,
          content: nativeFile.content,
          sizeBytes: nativeFile.sizeBytes,
          deleted: false,
          dirty: false,
          open: true,
          revision: documentRef.current.revision + 1
        };

        syncActiveDocumentFromEditor();

        const currentTabs = tabsRef.current;
        const existingTab = currentTabs.find((tab) => sameNativePath(tab.path, nativeFile.path));
        if (existingTab) return existingTab.id;

        const nextTab = createDocumentTab(nextDocument, fileTabId(nativeFile.path));
        const nextTabs = [...currentTabs, nextTab];

        tabsRef.current = nextTabs;
        setTabs(nextTabs);
        registerWindowRestoreState(activeFilePathFromTabs(nextTabs, activeTabIdRef.current), openFilePathsFromTabs(nextTabs));
        persistWorkspaceState({
          ...draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current),
          filePath: activeFilePathFromTabs(nextTabs, activeTabIdRef.current),
          openFilePaths: openFilePathsFromTabs(nextTabs)
        });

        return nextTab.id;
      } catch {
        return null;
      }
    },
    [readMarkdownFileWithPerformance, registerWindowRestoreState, syncActiveDocumentFromEditor]
  );

  const replaceOpenDocumentFile = useCallback((previousPath: string, file: NativeMarkdownFolderFile) => {
    const affected = tabsRef.current.some((tab) => tab.path === previousPath) || documentRef.current.path === previousPath;
    if (!affected) return false;

    const current = documentRef.current;
    if (current.path === previousPath) {
      setActiveDocument({
        ...current,
        deleted: false,
        name: file.name,
        path: file.path
      });
    }

    const nextTabs = tabsRef.current.map((tab) => {
      if (tab.path !== previousPath) return tab;

      return {
        ...tab,
        deleted: false,
        name: file.name,
        path: file.path
      };
    });
    tabsRef.current = nextTabs;
    setTabs(nextTabs);
    registerWindowRestoreState(activeFilePathFromTabs(nextTabs, activeTabIdRef.current), openFilePathsFromTabs(nextTabs));
    persistWorkspaceState({
      ...draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current),
      filePath: activeFilePathFromTabs(nextTabs, activeTabIdRef.current),
      openFilePaths: openFilePathsFromTabs(nextTabs)
    });
    return true;
  }, [registerWindowRestoreState, setActiveDocument]);

  const replaceMovedOpenDocumentFile = useCallback((
    previousPath: string,
    file: NativeMarkdownFolderFile,
    documentUpdate?: MovedMarkdownDocumentUpdate
  ) => {
    const movedPathFor = (path: string | null) => (path ? replaceMovedPath(path, previousPath, file.path) : path);
    const affected =
      tabsRef.current.some((tab) => tab.path !== null && movedPathFor(tab.path) !== tab.path) ||
      (documentRef.current.path !== null && movedPathFor(documentRef.current.path) !== documentRef.current.path);
    if (!affected) return false;

    const current = documentRef.current;
    if (current.path !== null) {
      const nextPath = movedPathFor(current.path);
      if (nextPath !== current.path) {
        const contentRebased = documentUpdate !== undefined && sameNativePath(current.path, previousPath);
        setActiveDocument({
          ...current,
          ...(contentRebased
            ? {
                content: documentUpdate.content,
                dirty: documentUpdate.dirty,
                revision: current.revision + 1
              }
            : {}),
          deleted: false,
          name: current.path === previousPath ? file.name : current.name,
          path: nextPath
        });
      }
    }

    const nextTabs = tabsRef.current.map((tab) => {
      const nextPath = movedPathFor(tab.path);
      if (nextPath === tab.path) return tab;

      const contentRebased = documentUpdate !== undefined && sameNativePath(tab.path, previousPath);
      if (contentRebased) editorSyncState.clearCleanVisualMarkdownBaseline(tab.id);
      return {
        ...tab,
        ...(contentRebased
          ? {
              content: documentUpdate.content,
              dirty: documentUpdate.dirty,
              revision: tab.revision + 1
            }
          : {}),
        deleted: false,
        name: tab.path === previousPath ? file.name : tab.name,
        path: nextPath
      };
    });
    tabsRef.current = nextTabs;
    setTabs(nextTabs);
    registerWindowRestoreState(activeFilePathFromTabs(nextTabs, activeTabIdRef.current), openFilePathsFromTabs(nextTabs));
    persistWorkspaceState({
      ...draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current),
      filePath: activeFilePathFromTabs(nextTabs, activeTabIdRef.current),
      openFilePaths: openFilePathsFromTabs(nextTabs)
    });
    return true;
  }, [editorSyncState, registerWindowRestoreState, setActiveDocument]);

  const detachDeletedDocumentFile = useCallback((path: string) => {
    const currentTabs = tabsRef.current;
    const deletedTab = currentTabs.find((tab) => tab.path !== null && isDeletedDocumentPath(tab.path, path));
    const currentDocumentPath = documentRef.current.path;
    if (!deletedTab && (!currentDocumentPath || !isDeletedDocumentPath(currentDocumentPath, path))) return false;

    const nextTabs = currentTabs.filter((tab) => tab.path === null || !isDeletedDocumentPath(tab.path, path));
    const deletedActiveTab =
      deletedTab?.id === activeTabIdRef.current ||
      (currentDocumentPath !== null && isDeletedDocumentPath(currentDocumentPath, path));

    if (deletedActiveTab) {
      const deletedIndex = currentTabs.findIndex((tab) => tab.path !== null && isDeletedDocumentPath(tab.path, path));
      const fallbackTab = nextTabs[Math.max(0, deletedIndex - 1)] ?? nextTabs[0] ?? null;
      setActiveTabState(nextTabs, fallbackTab?.id ?? null);
    } else {
      tabsRef.current = nextTabs;
      setTabs(nextTabs);
    }

    const nextDocumentPath = documentRef.current.path;
    if (!documentTabsEnabled && nextDocumentPath !== null && isDeletedDocumentPath(nextDocumentPath, path)) {
      const nextDocument = {
        content: "",
        dirty: false,
        name: "",
        open: false,
        path: null,
        revision: documentRef.current.revision + 1
      };
      setActiveDocument(nextDocument);
    }

    persistWorkspaceState({
      ...draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current),
      filePath: activeFilePathFromTabs(nextTabs, activeTabIdRef.current),
      openFilePaths: openFilePathsFromTabs(nextTabs)
    });
    registerWindowRestoreState(activeFilePathFromTabs(nextTabs, activeTabIdRef.current), openFilePathsFromTabs(nextTabs));
    return true;
  }, [documentTabsEnabled, registerWindowRestoreState, setActiveDocument, setActiveTabState]);

  const markExternallyDeletedDocumentFile = useCallback((path: string) => {
    const matchesDeletedPath = (documentPath: string | null) =>
      documentPath !== null && isDeletedDocumentPath(documentPath, path);
    const currentTabs = tabsRef.current;
    const current = documentRef.current;
    const activeTabId = activeTabIdRef.current;
    const affectsCurrent = matchesDeletedPath(current.path);
    const affectsTabs = currentTabs.some((tab) => matchesDeletedPath(tab.path));

    if (!affectsCurrent && !affectsTabs) return false;

    const nextDocument = affectsCurrent ? { ...current, deleted: true } : current;
    const nextTabs = currentTabs.map((tab) => {
      if (affectsCurrent && tab.id === activeTabId) return createDocumentTab(nextDocument, tab.id);
      if (matchesDeletedPath(tab.path)) return { ...tab, deleted: true };
      return tab;
    });

    tabsRef.current = nextTabs;
    setTabs(nextTabs);
    if (affectsCurrent) {
      documentRef.current = nextDocument;
      setDocument(nextDocument);
    }
    persistWorkspaceState({
      ...draftWorkspacePatchFromTabs(nextTabs, activeTabId),
      filePath: activeFilePathFromTabs(nextTabs, activeTabId),
      openFilePaths: openFilePathsFromTabs(nextTabs)
    });
    registerWindowRestoreState(activeFilePathFromTabs(nextTabs, activeTabId), openFilePathsFromTabs(nextTabs));
    return true;
  }, [registerWindowRestoreState]);

  const applySavedCurrentDocument = useCallback((
    savedFile: { name: string; path: string },
    contents: string,
    options: ApplySavedCurrentDocumentOptions = {}
  ) => {
    const currentTabs = tabsRef.current;
    const targetTabId = options.targetTabId ?? activeTabIdRef.current;
    const targetTab = targetTabId
      ? currentTabs.find((tab) => tab.id === targetTabId)
      : null;
    if (documentTabsEnabled && targetTabId && !targetTab) return;

    const fallbackTabId = targetTabId ?? activeTabIdRef.current ?? fileTabId(savedFile.path);
    const targetDocument =
      targetTabId === activeTabIdRef.current
        ? documentRef.current
        : targetTab
          ? documentFromTab(targetTab)
          : documentRef.current;
    const sourceContent = options.sourceContent ?? contents;
    const activeEditorMatchesSavedContent =
      targetTabId === activeTabIdRef.current && isActiveEditorMarkdownEquivalent(contents) === true;
    const contentChangedAfterSaveStarted =
      targetDocument.content !== sourceContent &&
      targetDocument.content !== contents &&
      !activeEditorMatchesSavedContent;
    const nextDocument = {
      ...targetDocument,
      path: savedFile.path,
      name: savedFile.name,
      content: contentChangedAfterSaveStarted ? targetDocument.content : contents,
      deleted: false,
      dirty: contentChangedAfterSaveStarted ? true : false,
      revision: targetDocument.revision
    };

    const nextTabs = documentTabsEnabled
      ? currentTabs.map((tab) => tab.id === fallbackTabId ? createDocumentTab(nextDocument, tab.id) : tab)
      : [createDocumentTab(nextDocument, fallbackTabId)];
    if (nextDocument.dirty) {
      editorSyncState.clear(fallbackTabId);
    } else {
      editorSyncState.rememberSavedVisualEditorStaleContent(fallbackTabId, contents);
    }
    tabsRef.current = nextTabs;
    setTabs(nextTabs);
    if (fallbackTabId === activeTabIdRef.current) {
      documentRef.current = nextDocument;
      setDocument(nextDocument);
    }

    const nextOpenFilePaths = documentTabsEnabled
      ? openFilePathsFromTabs(nextTabs)
      : [savedFile.path];
    const nextActiveFilePath = activeFilePathFromTabs(nextTabs, activeTabIdRef.current);
    if (options.retargetWorkspaceRoot) onTreeRootFromFilePath(savedFile.path);
    rememberRecentMarkdownFile(savedFile);
    registerWindowRestoreState(nextActiveFilePath, nextOpenFilePaths);
    persistWorkspaceState({
      ...draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current),
      filePath: nextActiveFilePath,
      openFilePaths: nextOpenFilePaths,
      ...(options.retargetWorkspaceRoot ? { folderName: null, folderPath: null } : {})
    });
  }, [
    documentTabsEnabled,
    editorSyncState,
    isActiveEditorMarkdownEquivalent,
    onTreeRootFromFilePath,
    persistWorkspaceState,
    registerWindowRestoreState,
    rememberRecentMarkdownFile
  ]);

  const saveCurrentDocument = useCallback(
    async (saveAs = false) => {
      const current = documentRef.current;
      if (!current.open) return null;

      const targetTabId = activeTabIdRef.current;
      const contents = currentMarkdownForSave(current, targetTabId);
      const savePath = saveAs || current.deleted ? null : current.path;
      const savedFile = await saveNativeMarkdownFile({
        ...defaultSaveDirectoryInput(defaultSaveDirectory, savePath),
        path: savePath,
        suggestedName: current.name || "Untitled.md",
        contents
      });

      if (!savedFile) return null;

      const selectedNewTarget = saveAs || current.path === null || current.deleted === true;
      if (await handoffPrimarySavedCopy(savedFile, selectedNewTarget)) return null;

      applySavedCurrentDocument(savedFile, contents, {
        retargetWorkspaceRoot: shouldRetargetSavedDocument(selectedNewTarget),
        sourceContent: current.content,
        targetTabId
      });
      return savedFile;
    },
    [
      applySavedCurrentDocument,
      currentMarkdownForSave,
      defaultSaveDirectory,
      handoffPrimarySavedCopy,
      shouldRetargetSavedDocument
    ]
  );

  const saveMarkdownTabContent = useCallback(
    async (
      tab: MarkdownDocumentTab,
      contents: string,
      options: SaveCurrentDocumentContentOptions = {}
    ) => {
      const savePath = tab.deleted ? null : tab.path;
      const savedFile = await saveNativeMarkdownFile({
        ...defaultSaveDirectoryInput(defaultSaveDirectory, savePath),
        historyCursorId: options.historyCursorId,
        path: savePath,
        skipHistorySnapshot: options.skipHistorySnapshot,
        suggestedName: tab.name || "Untitled.md",
        syncPathGuardRequestId: options.syncPathGuardRequestId,
        contents
      });

      if (!savedFile) return null;

      applySavedCurrentDocument(savedFile, contents, {
        retargetWorkspaceRoot: tab.path === null || tab.deleted === true,
        sourceContent: tab.content,
        targetTabId: tab.id
      });
      return savedFile;
    },
    [applySavedCurrentDocument, defaultSaveDirectory]
  );

  const saveCurrentDocumentContent = useCallback(
    async (contents: string, options: SaveCurrentDocumentContentOptions = {}) => {
      const current = documentRef.current;
      if (!current.open) {
        debug(() => ["[markra-history] save restored document ignored", {
          reason: "document closed"
        }]);
        return null;
      }

      const targetTabId = activeTabIdRef.current;
      debug(() => ["[markra-history] save restored document start", {
        contentsChars: contents.length,
        currentDirty: current.dirty,
        currentPath: current.path,
        currentRevision: current.revision,
        historyCursorId: options.historyCursorId ?? null,
        skipHistorySnapshot: options.skipHistorySnapshot === true,
        suggestedName: current.name || "Untitled.md"
      }]);

      const savePath = current.deleted ? null : current.path;
      let savedFile: Awaited<ReturnType<typeof saveNativeMarkdownFile>>;
      try {
        savedFile = await saveNativeMarkdownFile({
          ...defaultSaveDirectoryInput(defaultSaveDirectory, savePath),
          historyCursorId: options.historyCursorId,
          path: savePath,
          skipHistorySnapshot: options.skipHistorySnapshot,
          suggestedName: current.name || "Untitled.md",
          contents
        });
      } catch (error: unknown) {
        debug(() => ["[markra-history] save restored document native error", {
          currentPath: current.path,
          error: error instanceof Error ? error.message : String(error)
        }]);
        throw error;
      }

      if (!savedFile) {
        debug(() => ["[markra-history] save restored document canceled", {
          currentPath: current.path
        }]);
        return null;
      }

      applySavedCurrentDocument(savedFile, contents, {
        retargetWorkspaceRoot: current.path === null || current.deleted === true,
        sourceContent: current.content,
        targetTabId
      });
      debug(() => ["[markra-history] save restored document applied", {
        contentsChars: contents.length,
        savedPath: savedFile.path
      }]);
      return savedFile;
    },
    [applySavedCurrentDocument, defaultSaveDirectory]
  );

  const saveMarkdownTab = useCallback(
    async (tabId: string, saveAs = false) => {
      syncActiveDocumentFromEditor();

      const tab = tabsRef.current.find((candidate) => candidate.id === tabId);
      if (!tab?.open) return null;

      const contents = tab.id === activeTabIdRef.current
        ? currentMarkdownForSave(documentFromTab(tab), tab.id)
        : tab.content;
      const savePath = saveAs || tab.deleted ? null : tab.path;
      const savedFile = await saveNativeMarkdownFile({
        ...defaultSaveDirectoryInput(defaultSaveDirectory, savePath),
        path: savePath,
        suggestedName: tab.name || "Untitled.md",
        contents
      });

      if (!savedFile) return null;

      const selectedNewTarget = saveAs || tab.path === null || tab.deleted === true;
      if (await handoffPrimarySavedCopy(savedFile, selectedNewTarget)) return null;

      const sourceDocument = documentFromTab(tab);
      const nextDocument = {
        ...sourceDocument,
        path: savedFile.path,
        name: savedFile.name,
        content: contents,
        deleted: false,
        dirty: false,
        revision: sourceDocument.revision
      };
      const nextTabs = tabsRef.current.map((candidate) =>
        candidate.id === tabId ? createDocumentTab(nextDocument, candidate.id) : candidate
      );
      if (nextDocument.dirty) {
        editorSyncState.clear(tabId);
      } else {
        editorSyncState.rememberSavedVisualEditorStaleContent(tabId, contents);
      }

      tabsRef.current = nextTabs;
      setTabs(nextTabs);

      if (tab.id === activeTabIdRef.current) {
        documentRef.current = nextDocument;
        setDocument(nextDocument);
      }

      if (shouldRetargetSavedDocument(selectedNewTarget)) onTreeRootFromFilePath(savedFile.path);
      rememberRecentMarkdownFile(savedFile);
      registerWindowRestoreState(activeFilePathFromTabs(nextTabs, activeTabIdRef.current), openFilePathsFromTabs(nextTabs));
      persistWorkspaceState({
        ...draftWorkspacePatchFromTabs(nextTabs, activeTabIdRef.current),
        filePath: activeFilePathFromTabs(nextTabs, activeTabIdRef.current),
        openFilePaths: openFilePathsFromTabs(nextTabs),
        ...(shouldRetargetSavedDocument(selectedNewTarget) ? { folderName: null, folderPath: null } : {})
      });
      return savedFile;
    },
    [
      currentMarkdownForSave,
      defaultSaveDirectory,
      editorSyncState,
      handoffPrimarySavedCopy,
      onTreeRootFromFilePath,
      persistWorkspaceState,
      registerWindowRestoreState,
      rememberRecentMarkdownFile,
      shouldRetargetSavedDocument,
      syncActiveDocumentFromEditor
    ]
  );

  const handleSaveClick = useCallback(() => {
    saveCurrentDocument(false);
  }, [saveCurrentDocument]);

  const saveDirtyMarkdownFiles = useCallback(() => {
    if (dirtyFileSavePromiseRef.current) return dirtyFileSavePromiseRef.current;

    const operation = async () => {
      const snapshot = syncActiveDocumentDraftSnapshot();
      // Update relaunches can happen immediately; wait so untitled drafts survive the restart.
      await persistWorkspaceState(draftWorkspacePatchFromTabs(snapshot.tabs, snapshot.activeTabId));

      const dirtyTabs = snapshot.tabs.filter((tab) => tab.open && tab.dirty && tab.path !== null && !tab.deleted);
      const savedFiles: SavedNativeMarkdownFile[] = [];
      for (const tab of dirtyTabs) {
        const savedFile = await saveMarkdownTabContent(tab, tab.content, { skipHistorySnapshot: true });
        if (savedFile) savedFiles.push(savedFile);
      }
      return savedFiles;
    };
    const savePromise = dirtyFileWriteTailRef.current
      .then(operation, operation)
      .finally(() => {
      if (dirtyFileSavePromiseRef.current === savePromise) dirtyFileSavePromiseRef.current = null;
    });
    dirtyFileWriteTailRef.current = savePromise.then(
      () => undefined,
      () => undefined
    );

    dirtyFileSavePromiseRef.current = savePromise;
    return savePromise;
  }, [saveMarkdownTabContent, syncActiveDocumentDraftSnapshot]);

  const saveDirtyMarkdownPaths = useCallback((
    requestedPaths: readonly string[],
    syncPathGuardRequestId?: string
  ) => {
    const requested = requestedPaths
      .map(normalizeComparablePath)
      .filter((path): path is string => path !== null);
    if (requested.length === 0) return Promise.resolve(false);

    const operation = async () => {
      while (true) {
        const snapshot = syncActiveDocumentDraftSnapshot();
        const dirtyTabs = snapshot.tabs.filter((tab) => {
          if (!tab.open || !tab.dirty || tab.path === null || tab.deleted) return false;
          const path = normalizeComparablePath(tab.path);
          return path !== null && requested.includes(path);
        });
        if (dirtyTabs.length === 0) return true;

        for (const tab of dirtyTabs) {
          const savedFile = await saveMarkdownTabContent(tab, tab.content, {
            skipHistorySnapshot: true,
            syncPathGuardRequestId
          });
          if (!savedFile) return false;
        }
      }
    };
    const save = dirtyFileWriteTailRef.current.then(operation, operation);
    dirtyFileWriteTailRef.current = save.then(
      () => undefined,
      () => undefined
    );
    return save;
  }, [saveMarkdownTabContent, syncActiveDocumentDraftSnapshot]);

  const autoSaveDirtyMarkdownTabs = useCallback(async () => {
    try {
      await saveDirtyMarkdownFiles();
    } catch (error: unknown) {
      debug(() => ["[markra-autosave] save failed", {
        error: error instanceof Error ? error.message : String(error)
      }]);
    }
  }, [saveDirtyMarkdownFiles]);

  const selectMarkdownTab = useCallback((tabId: string) => {
    syncActiveDocumentFromEditor();
    const tab = tabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab) return false;

    setActiveTabState(tabsRef.current, tab.id);
    if (tab.path && !tab.dirty) refreshCleanOpenTabFromDisk(tab.path, "select-tab");
    registerWindowRestoreState(tab.path, openFilePathsFromTabs(tabsRef.current));
    persistWorkspaceState({
      ...draftWorkspacePatchFromTabs(tabsRef.current, tab.id),
      filePath: tab.path,
      openFilePaths: openFilePathsFromTabs(tabsRef.current)
    });
    return true;
  }, [refreshCleanOpenTabFromDisk, registerWindowRestoreState, setActiveTabState, syncActiveDocumentFromEditor]);

  const closeMarkdownTab = useCallback(async (tabId: string) => {
    syncActiveDocumentFromEditor();
    const currentTabs = tabsRef.current;
    const tabIndex = currentTabs.findIndex((tab) => tab.id === tabId);
    const tab = currentTabs[tabIndex];
    if (!tab) return false;

    if (hasDiscardableTabChanges(tab)) {
      const confirmed = await confirmDiscardUnsavedChanges?.(documentFromTab(tab));
      if (!confirmed) return false;
    }

    const nextTabs = currentTabs.filter((candidate) => candidate.id !== tabId);
    const nextActiveTab =
      tab.id === activeTabIdRef.current
        ? nextTabs[Math.max(0, tabIndex - 1)] ?? nextTabs[0] ?? null
        : nextTabs.find((candidate) => candidate.id === activeTabIdRef.current) ?? null;

    editorSyncState.clear(tabId);
    setActiveTabState(nextTabs, nextActiveTab?.id ?? null);
    registerWindowRestoreState(nextActiveTab?.path ?? null, openFilePathsFromTabs(nextTabs));
    persistWorkspaceState({
      ...draftWorkspacePatchFromTabs(nextTabs, nextActiveTab?.id ?? null),
      filePath: nextActiveTab?.path ?? null,
      openFilePaths: openFilePathsFromTabs(nextTabs)
    });
    return true;
  }, [
    confirmDiscardUnsavedChanges,
    editorSyncState,
    hasDiscardableTabChanges,
    registerWindowRestoreState,
    setActiveTabState,
    syncActiveDocumentFromEditor
  ]);

  const isCurrentDocumentEmptyUntitled = useCallback(() => {
    const current = documentRef.current;
    return !current.open || (current.path === null && currentMarkdown().trim() === "");
  }, [currentMarkdown]);

  const isFileInCurrentWorkspace = useCallback((path: string) => {
    const workspaceRootPath = workspaceRootForSource(workspaceSourcePath, documentRef.current.path);
    return isPathWithinRoot(path, workspaceRootPath);
  }, [workspaceSourcePath]);

  const handleDroppedMarkdownPath = useCallback(
    async (target: NativeMarkdownDroppedTarget) => {
      const workspaceRootPath = workspaceRootForSource(workspaceSourcePath, documentRef.current.path);
      if (
        target.kind !== "image" &&
        workspaceRootPath &&
        !isPathWithinRoot(target.path, workspaceRootPath)
      ) {
        // Keep each window bound to one workspace; tab reuse only applies to paths inside it.
        if (target.kind === "folder") {
          await openNativeMarkdownFolderInNewWindow(target.path);
        } else {
          await openNativeMarkdownFileInNewWindow(target.path);
        }
        return;
      }

      if (target.kind === "folder") {
        if (!onSwitchNotebookDirectory) return false;
        const switchedRoot = await onSwitchNotebookDirectory(target.path);
        return switchedRoot !== null && switchedRoot !== false;
      }

      const currentDocumentEmpty = isCurrentDocumentEmptyUntitled();
      const hasExistingWindowContext =
        Boolean(normalizeComparablePath(workspaceSourcePath)) ||
        tabsRef.current.some((tab) =>
          tab.id !== activeTabIdRef.current &&
          !isEmptyUntitledDocument(documentFromTab(tab))
        );

      if (
        openDroppedFilesInTabs &&
        documentTabsEnabled &&
        (!currentDocumentEmpty || hasExistingWindowContext)
      ) {
        // An empty active tab can still belong to a window with a workspace or other documents.
        await loadNativeMarkdownPath(target.path, false);
        return true;
      }

      if (currentDocumentEmpty) {
        await loadNativeMarkdownPath(target.path);
        return true;
      }

      if (resolvedNativeOpenPolicy === "spawn-external") {
        await openNativeMarkdownFileInNewWindow(target.path);
        return true;
      }

      await openNativeMarkdownFileInNewWindow(target.path);
      return true;
    },
    [
      documentTabsEnabled,
      isCurrentDocumentEmptyUntitled,
      loadNativeMarkdownPath,
      onSwitchNotebookDirectory,
      openDroppedFilesInTabs,
      resolvedNativeOpenPolicy,
      workspaceSourcePath
    ]
  );

  const handleNativeOpenedMarkdownPaths = useCallback(async (paths: string[]) => {
    if (resolvedNativeOpenPolicy === "managed") return false;
    if (paths.length === 0) return false;

    let openedPath = false;

    for (const path of paths) {
      try {
        const target = await resolveNativeMarkdownPath(path);

        if (target.kind === "folder") {
          const switched = await handleDroppedMarkdownPath(target);
          if (switched) openedPath = true;
          continue;
        }

        if (resolvedNativeOpenPolicy === "spawn-external") {
          await openNativeMarkdownFileInNewWindow(target.path);
          openedPath = true;
          continue;
        }

        if (isCurrentDocumentEmptyUntitled() || (documentTabsEnabled && isFileInCurrentWorkspace(target.path))) {
          await loadNativeMarkdownPath(target.path);
        } else {
          await openNativeMarkdownFileInNewWindow(target.path);
        }

        openedPath = true;
      } catch {
        // Unsupported or moved OS-opened paths should not block other files.
      }
    }

    if (openedPath && resolvedNativeOpenPolicy !== "spawn-external") {
      openedFromNativeRef.current = true;
    }
    return openedPath;
  }, [documentTabsEnabled, handleDroppedMarkdownPath, isCurrentDocumentEmptyUntitled, isFileInCurrentWorkspace, loadNativeMarkdownPath, resolvedNativeOpenPolicy]);

  useEffect(() => {
    const title = document.open && document.dirty ? `${document.name} *` : document.name;
    setNativeWindowTitle(title);
  }, [document.name, document.dirty, document.open]);

  useEffect(() => {
    const handleBeforeUnload = (event: BeforeUnloadEvent) => {
      syncActiveDocumentFromEditor();
      const hasUnsavedChanges = tabsRef.current.some((tab) => hasDiscardableTabChanges(tab));
      if (!hasUnsavedChanges) return;

      event.preventDefault();
      event.returnValue = "";
    };

    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => {
      window.removeEventListener("beforeunload", handleBeforeUnload);
    };
  }, [hasDiscardableTabChanges, syncActiveDocumentFromEditor]);

  useEffect(() => {
    if (!autoSaveEnabled) return;

    const intervalMinutes = Number.isFinite(autoSaveIntervalMinutes)
      ? Math.max(1, Math.round(autoSaveIntervalMinutes))
      : 10;
    const intervalMs = intervalMinutes * 60_000;
    const timer = window.setInterval(() => {
      autoSaveDirtyMarkdownTabs().catch(() => {});
    }, intervalMs);

    return () => {
      window.clearInterval(timer);
    };
  }, [autoSaveDirtyMarkdownTabs, autoSaveEnabled, autoSaveIntervalMinutes]);

  useEffect(() => {
    let active = true;

    getStoredRecentMarkdownFiles().then((files) => {
      if (active) setRecentFiles(files);
    }).catch(() => {});

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    let cleanup: (() => unknown) | null = null;

    listenNativeWindowCloseRequested(async (event) => {
      event.preventDefault();
      if (nativeCloseBlockedRef.current) return;
      if (nativeWindowCloseScheduledRef.current) return;

      const draftPersistence = persistActiveDocumentDraftSnapshot();
      const canDiscard = await confirmCanDiscardCurrentDocument();
      if (!canDiscard) return;

      await draftPersistence;
      nativeWindowCloseScheduledRef.current = true;
      window.setTimeout(() => {
        // The close request was already intercepted for persistence. Destroy the window after
        // saving. The native command coordinates with an owned Settings window and may leave the
        // editor open when Settings cancels its hide, so a later close request must be allowed to
        // run through this interception path again.
        destroyNativeWindow().then(
          () => {
            nativeWindowCloseScheduledRef.current = false;
          },
          () => {
            nativeWindowCloseScheduledRef.current = false;
          }
        );
      }, 0);
    }).then((nextCleanup) => {
      if (active) {
        cleanup = nextCleanup;
        return;
      }

      nextCleanup();
    }).catch(() => {});

    return () => {
      active = false;
      cleanup?.();
    };
  }, [confirmCanDiscardCurrentDocument, persistActiveDocumentDraftSnapshot]);

  useEffect(() => {
    let active = true;
    let cleanup: (() => unknown) | null = null;

    listenNativeAppExitRequested(requestAppExit).then((nextCleanup) => {
      if (active) {
        cleanup = nextCleanup;
        return;
      }

      nextCleanup();
    }).catch(() => {});

    return () => {
      active = false;
      cleanup?.();
    };
  }, [requestAppExit]);

  const externalFilePath = windowContext.kind === "external-file"
    ? windowContext.path
    : null;

  useEffect(() => {
    if (managedWorkspace) return;
    if (!externalFilePath) return;
    if (externalFileStartupPathRef.current === externalFilePath) return;
    externalFileStartupPathRef.current = externalFilePath;

    let active = true;
    let settled = false;
    setWorkspaceSurface("restoring");

    readMarkdownFileWithPerformance(externalFilePath, "initial-path").then((file) => {
      if (!active) return;
      settled = true;
      applyNativeMarkdownFile(file, false);
    }).catch(() => {
      if (!active) return;
      settled = true;
      clearOpenDocument({ persistWorkspace: false });
    });

    return () => {
      active = false;
      if (
        externalFileStartupPathRef.current === externalFilePath &&
        !settled
      ) {
        externalFileStartupPathRef.current = null;
      }
    };
  }, [applyNativeMarkdownFile, clearOpenDocument, externalFilePath, managedWorkspace, readMarkdownFileWithPerformance]);

  const externalFolderPath = windowContext.kind === "external-folder"
    ? windowContext.path
    : null;

  useEffect(() => {
    if (managedWorkspace || !externalFolderPath) return;
    if (externalFolderStartupPathRef.current === externalFolderPath) return;
    externalFolderStartupPathRef.current = externalFolderPath;
    let active = true;
    setWorkspaceSurface("restoring");

    Promise.resolve().then(() => onTreeRootFromFolderPath(
      externalFolderPath,
      pathNameFromPath(externalFolderPath),
      true,
      true
    )).then(() => {
      if (active) clearOpenDocument({ persistWorkspace: false });
    }).catch(() => {
      if (active) clearOpenDocument({ persistWorkspace: false });
    });

    return () => {
      active = false;
    };
  }, [clearOpenDocument, externalFolderPath, managedWorkspace, onTreeRootFromFolderPath]);

  useEffect(() => {
    if (managedWorkspace || windowContext.kind !== "external-blank") return;
    setWorkspaceSurface(workspaceReady ? "editor" : "recovery");
  }, [managedWorkspace, windowContext.kind, workspaceReady]);

  useEffect(() => {
    if (resolvedNativeOpenPolicy === "managed") {
      setNativeOpenedPathsReady(true);
      return;
    }

    if (windowContext.kind !== "primary") {
      setNativeOpenedPathsReady(true);
      return;
    }

    let active = true;
    let cleanupNativeListener: (() => unknown) | null = null;

    if (nativeOpenedPathsInitializationRef.current === null) {
      nativeOpenedPathsInitializationRef.current = (async () => {
        try {
          const paths = await takeNativeOpenedMarkdownPaths();
          return await handleNativeOpenedMarkdownPaths(paths);
        } catch {
          // Native launch state is opportunistic; the normal startup path remains available.
          return false;
        }
      })();
    }

    const nativeOpenedPathsInitialization = nativeOpenedPathsInitializationRef.current;

    (async () => {
      await nativeOpenedPathsInitialization;
      if (!active) return;

      setNativeOpenedPathsReady(true);

      try {
        const cleanup = await listenNativeOpenedMarkdownPaths((paths) => {
          takeNativeOpenedMarkdownPaths()
            .then((queuedPaths) => queuedPaths.length > 0
              ? handleNativeOpenedMarkdownPaths(queuedPaths)
              : false)
            .catch(() => handleNativeOpenedMarkdownPaths(paths));
        });

        if (!active) {
          cleanup();
          return;
        }

        cleanupNativeListener = cleanup;
      } catch {
        // If event registration fails, manual open and drag/drop still work.
      }

      if (!active) return;
      try {
        const paths = await takeNativeOpenedMarkdownPaths();
        await handleNativeOpenedMarkdownPaths(paths);
      } catch {
        // A failed follow-up drain leaves the native queue available for the next startup.
      }
    })().catch(() => {
      if (active) setNativeOpenedPathsReady(true);
    });

    return () => {
      active = false;
      cleanupNativeListener?.();
    };
  }, [handleNativeOpenedMarkdownPaths, resolvedNativeOpenPolicy, windowContext.kind]);

  useEffect(() => {
    if (windowContext.kind !== "primary") return;
    if (!workspaceReady) return;
    if (!nativeOpenedPathsReady) return;
    if (openedFromNativeRef.current) return;
    if (!preferencesReady) return;
    const restoreKey = `${windowContext.kind}:${restoreWorkspaceRoot ?? ""}:${String(restoreWorkspaceOnStartup)}`;
    if (startupWorkspaceRestoreKeyRef.current === restoreKey) return;
    startupWorkspaceRestoreKeyRef.current = restoreKey;

    let active = true;
    let settled = false;
    setWorkspaceSurface("restoring");

    if (restoreWorkspaceRoot) {
      const retainedTabs = tabsRef.current.filter((tab) =>
        tab.path === null || documentPathIsWithinWorkspaceRoot(restoreWorkspaceRoot, tab.path)
      );
      if (retainedTabs.length !== tabsRef.current.length) {
        if (retainedTabs.length === 0) {
          clearOpenDocument({ persistWorkspace: false });
        } else {
          const retainedActiveTabId = activeTabIdRef.current && retainedTabs.some(
            (tab) => tab.id === activeTabIdRef.current
          )
            ? activeTabIdRef.current
            : retainedTabs.at(-1)?.id ?? null;
          setActiveTabState(retainedTabs, retainedActiveTabId);
        }
      }
    }

    (async () => {
      if (!restoreWorkspaceOnStartup) {
        clearOpenDocument({ persistWorkspace: false });
        return;
      }

      try {
          const workspace = await getStoredWorkspaceState();
          if (!active) return;
          const restoreWindows = workspace.openWindows ?? [];
          const currentWindowHasRestoreState = workspaceHasCurrentWindowRestoreState(workspace);
          const primaryRestoreWindow = currentWindowHasRestoreState ? null : restoreWindows[0] ?? null;
          const primaryRestoreWindowFilePath = primaryRestoreWindow
            ? activeFilePathFromWindowRestore(primaryRestoreWindow)
            : null;
          let restoreFilePaths = primaryRestoreWindow
            ? restoreFilePathsFromWorkspace(
              primaryRestoreWindow.openFilePaths,
              primaryRestoreWindowFilePath,
              documentTabsEnabled
            )
            : restoreFilePathsFromWorkspace(
              workspace.openFilePaths,
              workspace.filePath,
              documentTabsEnabled
            );
          if (restoreWorkspaceRoot) {
            restoreFilePaths = restoreFilePaths.filter(
              (path) => documentPathIsWithinWorkspaceRoot(restoreWorkspaceRoot, path)
            );
          }
          const requestedActiveRestoreFilePath = primaryRestoreWindow
            ? primaryRestoreWindowFilePath
            : workspace.filePath;
          const activeRestoreFilePath = requestedActiveRestoreFilePath && (
            !restoreWorkspaceRoot ||
            documentPathIsWithinWorkspaceRoot(restoreWorkspaceRoot, requestedActiveRestoreFilePath)
          )
            ? requestedActiveRestoreFilePath
            : null;
          const additionalRestoreWindowFilePaths = normalizeOpenFilePaths(
            additionalEditorWindowsForRestore(restoreWindows, currentWindowHasRestoreState)
              .map(activeFilePathFromWindowRestore)
          ).filter((path) =>
            !restoreFilePaths.includes(path) && (
              !restoreWorkspaceRoot || documentPathIsWithinWorkspaceRoot(restoreWorkspaceRoot, path)
            )
          );
          const restoredDraftTabs = restoreWorkspaceRoot
            ? (workspace.draftTabs ?? []).filter((draft) =>
              draft.path === null || documentPathIsWithinWorkspaceRoot(restoreWorkspaceRoot, draft.path)
            )
            : workspace.draftTabs ?? [];
          const restoredActiveDraftId = workspace.activeDraftId && restoredDraftTabs.some(
            (draft) => draft.id === workspace.activeDraftId
          )
            ? workspace.activeDraftId
            : null;
          let folderRestorePromise: Promise<unknown> | null = null;
          if (workspace.folderPath && !restoreWorkspaceRoot) {
            const folderPath = workspace.folderPath;
            const folderName = workspace.folderName ?? folderPath;
            const restoreFolderRoot = () => onTreeRootFromFolderPath(
              folderPath,
              folderName,
              restoreFilePaths.length === 0 && restoredDraftTabs.length === 0,
              workspace.fileTreeOpen,
              { persistWorkspace: false }
            );
            folderRestorePromise = Promise.resolve().then(restoreFolderRoot).catch(() => null);
          }

          const restoredFiles = restoreFilePaths.length > 0
            ? await restoreNativeMarkdownFiles(
              restoreFilePaths,
              activeRestoreFilePath,
              !restoreWorkspaceRoot && !workspace.folderPath,
              () => active
            )
            : {
              activeFilePath: null,
              cancelled: false,
              failedPaths: [],
              files: [],
              survivingPaths: []
            } satisfies NativeMarkdownFilesRestoreResult;
          if (!active || restoredFiles.cancelled) return;
          const restoredDrafts = restoreWorkspaceDraftTabs(
            restoredDraftTabs,
            restoredActiveDraftId
          );
          if (!active) return;

          const settledSurface = workspaceSurfaceForRestore({
            recoverableDraftCount: restoredDrafts.recoverableDraftCount,
            validFileCount: restoredFiles.survivingPaths.length,
            workspaceReady: workspaceReadyRef.current
          });
          if (settledSurface === "editor") {
            setWorkspaceSurface("editor");
          }

          for (const path of managedWorkspace ? [] : additionalRestoreWindowFilePaths) {
            if (!active) return;
            try {
              await openNativeMarkdownFileInNewWindow(path);
            } catch {
              // A secondary window failure should not change the current workspace surface.
            }
            if (!active) return;
          }

          const folderResult = folderRestorePromise ? await folderRestorePromise : null;
          if (!active) return;

          if (settledSurface !== "editor") {
            clearOpenDocument({ persistWorkspace: false });
          }

          const finalTabs = tabsRef.current;
          const finalActiveTabId = activeTabIdRef.current;
          const finalOpenFilePaths = openFilePathsFromTabs(finalTabs);
          const finalActiveFilePath = activeFilePathFromTabs(finalTabs, finalActiveTabId);
          const finalSideBySideGroup = workspace.sideBySideGroup &&
            finalOpenFilePaths.includes(workspace.sideBySideGroup.primaryFilePath) &&
            finalOpenFilePaths.includes(workspace.sideBySideGroup.sideFilePath)
            ? workspace.sideBySideGroup
            : null;
          const restoredFolderFailed = Boolean(workspace.folderPath) &&
            !restoreWorkspaceRoot &&
            (folderResult === null || folderResult === false);
          const layoutHadRememberedState = Boolean(
            workspace.filePath ||
            workspace.folderPath ||
            workspace.openFilePaths.length > 0 ||
            (workspace.draftTabs?.length ?? 0) > 0 ||
            restoreWindows.length > 0 ||
            workspace.sideBySideGroup
          );

          if (layoutHadRememberedState) {
            persistWorkspaceState({
              ...draftWorkspacePatchFromTabs(finalTabs, finalActiveTabId),
              filePath: finalActiveFilePath,
              openFilePaths: finalOpenFilePaths,
              openWindows: [],
              sideBySideGroup: finalSideBySideGroup,
              ...(restoreWorkspaceRoot || restoredFolderFailed
                ? {
                  folderName: null,
                  folderPath: null,
                  ...(restoredFolderFailed ? { fileTreeOpen: false } : {})
                }
                : {})
            });
          }
      } catch {
        if (!active) return;
        clearOpenDocument({ persistWorkspace: false });
      }
    })().then(
      () => {
        if (active) settled = true;
      },
      () => {
        if (active) settled = true;
      }
    );

    return () => {
      active = false;
      if (!settled && startupWorkspaceRestoreKeyRef.current === restoreKey) {
        startupWorkspaceRestoreKeyRef.current = null;
      }
    };
  }, [
    clearOpenDocument,
    documentTabsEnabled,
    managedWorkspace,
    nativeOpenedPathsReady,
    onTreeRootFromFolderPath,
    persistWorkspaceState,
    preferencesReady,
    restoreNativeMarkdownFiles,
    restoreWorkspaceDraftTabs,
    restoreWorkspaceOnStartup,
    restoreWorkspaceRoot,
    setActiveTabState,
    workspaceReady,
    windowContext.kind
  ]);

  useEffect(() => {
    const watchedPaths = watchedMarkdownFilePathsKey.split("\n").filter((path) => path.trim().length > 0);
    if (watchedPaths.length === 0) return;
    const ignoreRootPath = workspaceRootForSource(workspaceSourcePath, document.path);

    let active = true;
    const stopWatchers: Array<() => unknown> = [];
    const watchedFileReadStates = new Map<string, WatchedMarkdownFileReadState>();

    const readAndApplyWatchedFile = async (changedPath: string, watchedPath: string) => {
      debug(() => ["[markra-history] watcher file event", {
        changedPath,
        watchedPath
      }]);
      let file: NativeMarkdownFile;
      try {
        file = await readMarkdownFileWithPerformance(changedPath, "watcher");
      } catch (error: unknown) {
        if (active && isMissingMarkdownFileReadError(error)) {
          const markedDeleted = markExternallyDeletedDocumentFile(changedPath);
          debug(() => ["[markra-history] watcher marked missing file", {
            changedPath,
            markedDeleted,
            watchedPath
          }]);
          onMarkdownTreeChange?.(changedPath);
          return;
        }

        debug(() => ["[markra-history] watcher read failed", {
          changedPath,
          error: error instanceof Error ? error.message : String(error),
          watchedPath
        }]);
        return;
      }
      if (!active) return;

      applyDiskFileToCleanOpenTab(file, "watcher");
    };

    const drainWatchedFileRead = async (watchedPath: string, state: WatchedMarkdownFileReadState) => {
      try {
        while (active) {
          state.pending = false;
          await readAndApplyWatchedFile(state.changedPath, watchedPath);
          if (!state.pending) break;
        }
      } finally {
        if (watchedFileReadStates.get(watchedPath) === state) {
          watchedFileReadStates.delete(watchedPath);
        }
      }
    };

    const requestWatchedFileRead = (changedPath: string, watchedPath: string) => {
      const existingState = watchedFileReadStates.get(watchedPath);
      if (existingState) {
        existingState.changedPath = changedPath;
        existingState.pending = true;
        debug(() => ["[markra-history] watcher file event coalesced", {
          changedPath,
          watchedPath
        }]);
        return existingState.promise ?? Promise.resolve();
      }

      const state: WatchedMarkdownFileReadState = {
        changedPath,
        pending: false,
        promise: null
      };
      watchedFileReadStates.set(watchedPath, state);
      const promise = drainWatchedFileRead(watchedPath, state);
      state.promise = promise;
      return promise;
    };

    watchedPaths.forEach((watchedPath) => {
      debug(() => ["[markra-history] watcher start", {
        path: watchedPath
      }]);

      const treeChangeHandler = sameNativePath(watchedPath, document.path)
        ? (changedPath: string) => {
            if (!active) return;
            debug(() => ["[markra-history] watcher tree event", {
              changedPath,
              watchedPath
            }]);
            onMarkdownTreeChange?.(changedPath);
          }
        : undefined;

      watchNativeMarkdownFile(watchedPath, async (changedPath) => {
        if (!active) return;

        await requestWatchedFileRead(changedPath, watchedPath);
      }, treeChangeHandler, { globalIgnoreRules, ignoreRootPath }).then((stopWatching) => {
        if (!active) {
          stopWatching();
          return;
        }

        stopWatchers.push(stopWatching);
        debug(() => ["[markra-history] watcher ready", {
          path: watchedPath
        }]);
      }).catch((error: unknown) => {
        debug(() => ["[markra-history] watcher failed", {
          error: error instanceof Error ? error.message : String(error),
          path: watchedPath
        }]);
      });
    });

    return () => {
      active = false;
      watchedPaths.forEach((watchedPath) => {
        debug(() => ["[markra-history] watcher stop", {
          path: watchedPath
        }]);
      });
      stopWatchers.forEach((stopWatching) => stopWatching());
    };
  }, [
    applyDiskFileToCleanOpenTab,
    document.path,
    globalIgnoreRules,
    markExternallyDeletedDocumentFile,
    onMarkdownTreeChange,
    readMarkdownFileWithPerformance,
    workspaceSourcePath,
    watchedMarkdownFilePathsKey
  ]);

  return {
    clearRecentMarkdownFiles,
    clearOpenDocument,
    closeMarkdownTab,
    createBlankDocument,
    confirmCanDiscardCurrentDocument,
    detachDeletedDocumentFile,
    document,
    workspaceSurface,
    tabs,
    activeTabId,
    handleDroppedMarkdownPath,
    getDirtyMarkdownFileContent,
    handleMarkdownChange,
    handleMarkdownTabChange,
    handleSaveClick,
    rememberMarkdownTabVisualBaseline,
    openMarkdownFile,
    openRecentMarkdownFile,
    openTreeMarkdownFileInBackground,
    openTreeMarkdownFile,
    outlineItems,
    replaceOpenDocumentFile,
    replaceMovedOpenDocumentFile,
    requestAppExit,
    recentFiles,
    restoreDocumentContent,
    saveCurrentDocumentContent,
    saveCurrentDocument,
    saveDirtyMarkdownFiles,
    saveDirtyMarkdownPaths,
    saveMarkdownTab,
    selectMarkdownTab,
    wordCount
  };
}
