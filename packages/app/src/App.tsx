import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent as ReactDragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type UIEvent as ReactUIEvent
} from "react";
import { AppToaster } from "./components/AppToaster";
import { SelectionToolbar } from "./components/SelectionToolbar";
import { DocumentHistoryDialog } from "./components/DocumentHistoryDialog";
import { AssetCleanupDialog } from "./components/AssetCleanupDialog";
import { DocumentSearchBar } from "./components/DocumentSearchBar";
import { GlobalSearchPanel } from "./components/GlobalSearchPanel";
import { ImagePreview } from "./components/ImagePreview";
import { LargeMarkdownNotice } from "./components/LargeMarkdownNotice";
import {
  MarkdownExportDocument,
  type RenderedMarkdownExport,
  type MarkdownExportSnapshot
} from "./components/MarkdownExportDocument";
import { MarkdownPaper } from "./components/MarkdownPaper";
import { LazyMarkdownSourceEditor } from "./components/LazyMarkdownSourceEditor";
import {
  MarkdownTabsBar,
  markdownTabDragDataType,
  type MarkdownTabsBarDocumentItem
} from "./components/MarkdownTabsBar";
import { NativeTitleBar } from "./components/NativeTitleBar";
import { QuietStatus } from "./components/QuietStatus";
import { QuickOpenPanel } from "./components/QuickOpenPanel";
import { SettingsWindowLoadingShell } from "./components/SettingsWindowLoadingShell";
import { SideDocumentPane } from "./components/SideDocumentPane";
import type { SidebarSyncButtonState } from "./components/SidebarSyncButton";
import { WorkspaceLayout } from "./components/WorkspaceLayout";
import { CompactAppShell } from "./components/compact/CompactAppShell";
import { MobileNotebookDialog } from "./components/notebooks/MobileNotebookDialog";
import { RemoteNotebookDialog } from "./components/notebooks/RemoteNotebookDialog";
import { WelcomeScreen } from "./components/onboarding/WelcomeScreen";
import type { CompactAppController } from "./components/compact/types";
import { useAppLanguage } from "./hooks/useAppLanguage";
import { useAppTheme } from "./hooks/useAppTheme";
import { useDocumentSearchState } from "./hooks/useDocumentSearchState";
import { useEditorContentWidthState } from "./hooks/useEditorContentWidthState";
import { useEditorPreferences } from "./hooks/useEditorPreferences";
import { useExportSettings } from "./hooks/useExportSettings";
import { useFileIgnoreSettings } from "./hooks/useFileIgnoreSettings";
import { useCompactMode } from "./hooks/useCompactMode";
import { useCompactAutoSave } from "./hooks/useCompactAutoSave";
import { usePrimaryWorkspace } from "./hooks/usePrimaryWorkspace";
import { useCodeMirrorEditorController } from "./hooks/useCodeMirrorEditorController";
import { shouldFocusEditorOnReady } from "./lib/editor-focus";
import { useMarkdownDocument, type ActiveDiskFileContentChange } from "./hooks/useMarkdownDocument";
import { useMarkdownFileTree } from "./hooks/useMarkdownFileTree";
import { useCompactSyncSettings } from "./hooks/useCompactSyncSettings";
import { useAppSyncCoordinator } from "./hooks/useAppSyncCoordinator";
import { useNotebookSwitchCoordinator } from "./hooks/useNotebookSwitchCoordinator";
import { useSyncConfig } from "./hooks/useSyncConfig";
import { useSyncPathGuard } from "./hooks/useSyncPathGuard";
import { useSyncConflictHistory } from "./hooks/useSyncConflictHistory";
import { useSelectionToolbarAnchorRefresh } from "./hooks/useSelectionToolbarAnchorRefresh";
import { useSharedEditorHistory } from "./hooks/useSharedEditorHistory";
import { routeMarkdownChangeToTab } from "./hooks/markdown-document/editor-sync";
import { useSideBySideTabs } from "./hooks/useSideBySideTabs";
import { useAutoUpdater } from "./hooks/useAutoUpdater";
import { useDefaultContextMenuBlocker } from "./hooks/useDefaultContextMenuBlocker";
import { useWorkspaceLinkIndex } from "./hooks/useWorkspaceLinkIndex";
import { useSettingsWindowRoute } from "./hooks/useSettingsWindowRoute";
import { useRuntimeLogCapture } from "./hooks/useRuntimeLogCapture";
import { useStartupWindowReveal } from "./hooks/useStartupWindowReveal";
import { useWorkspaceSearch } from "./hooks/useWorkspaceSearch";
import { useWorkspaceResourceSnapshotResponder } from "./hooks/useWorkspaceResourceSnapshotResponder";
import { useWorkspaceAssetCleanup } from "./hooks/useWorkspaceAssetCleanup";
import {
  useApplicationShortcuts,
  useNativeMarkdownDrop,
  useNativeMenuHandlers,
  useNativeMenus,
  useSettingsWindowShortcut
} from "./hooks/useNativeBindings";
import type { EditorView } from "@codemirror/view";
import {
  clampNumber,
  debug,
  diagnosticErrorMessage,
  markdownImageDragPayloadForFile,
  markdownImageDragSrcForDocument,
  pathNameFromPath,
  t,
  type AppLanguage,
  type I18nKey,
  type MarkdownFormattingShortcutAction
} from "@markra/shared";
import { showAppToast } from "./lib/app-toast";
import { appVersion } from "./lib/app-version";
import { createMarkdownImageSrcResolver, getWordCount } from "@markra/markdown";
import { buildMarkdownHtmlDocument, exportDocumentFileName, localFileUrlFromPath } from "./lib/document-export";
import {
  generateCrashDiagnosticsReport,
  generateDiagnosticsIssueUrl
} from "./lib/diagnostics/diagnostics-report";
import { resolveMarkdownDocumentLinkFile, resolveMarkdownDocumentLinkPath } from "./lib/document-links";
import { createImageUploadFileName, saveLocalEditorImage } from "./lib/image-upload";
import {
  persistRemoteEditorImage,
  resolveEditorAssetAction,
  resolveEditorAssetContext
} from "./lib/editor-assets";
import { shouldBlockLargeMarkdownVisual } from "./lib/large-markdown";
import { markAppPerformance } from "./lib/performance-marks";
import {
  moveMarkdownTreeFileWithLinks,
  type MarkdownTreeMoveDocumentUpdate
} from "./lib/markdown-tree-move";
import { replaceMovedPath, sameNativePath } from "./lib/path-move";
import {
  editorReadOnlyForPath,
  editorReadOnlyForTarget,
  syncExistingDocumentWriteBlockedByGuardedPath,
  syncSaveBlockedByGuardedPath,
  syncMutationIntersectsGuardedPaths
} from "./lib/sync-path-events";
import { SyncPathMutationRegistry } from "./lib/sync-path-mutations";
import { nextViewMode, resolveViewModeChrome, type ViewMode } from "./lib/view-mode";
import {
  resolveDesktopOsVersion,
  resolveDesktopPlatform,
  webKitScrollWorkaroundForPlatform
} from "./lib/platform";
import { selectionAnchorFromDomSelection, type SelectionAnchor } from "./lib/selection-anchor";
import { runEditorLinkCommand } from "./app/editor-link-command";
import {
  isExternalEditorWindow,
  parseEditorWindowContext
} from "./lib/editor-window-context";
import { requestPrimaryNotebookSwitch } from "./lib/notebook-switch-events";
import {
  listenPrimaryCloudNotebookRestoreRequested,
  type PrimaryCloudNotebookRestoreRequest
} from "./lib/cloud-notebook-restore-events";
import { isPandocSetupError, runPandocSetupAction } from "./app/pandoc-setup";
import type { WorkspaceSearchResult } from "./lib/workspace-search";
import type {
  SelectionHeadingLevel,
  SelectionFormattingAction,
  SelectionFormattingToolbarAction
} from "./lib/selection-formatting";
import {
  closeNativeWindow,
  hideSettingsWindow,
  listenNativeApplicationMenuCommands,
  openBlankEditorWindow,
  openNativeExternalUrl,
  showNativeAppAbout,
  toggleNativeWindowFullscreen,
  toggleNativeWindowMaximized
} from "./lib/tauri";
import {
  createEditorResourceRequest,
  markdownShortcutToKeyboardEventInit,
  normalizeEditorImageFile,
  normalizeMarkdownShortcuts,
  type EditorTextSelection,
  type RemoteClipboardImage,
  type SaveEditorResources
} from "@markra/editor";
import {
  defaultSplitVisualPanePercent,
  getStoredWorkspaceState,
  splitVisualPanePercentMax,
  splitVisualPanePercentMin,
  saveStoredEditorPreferences,
  saveStoredWorkspaceState,
  type RecentMarkdownFile,
  type EditorPreferences,
  type StoredWorkspaceSideBySideGroup,
  type TitlebarActionPreference
} from "./lib/settings/app-settings";
import {
  notifyAppEditorPreferencesChanged
} from "./lib/settings/settings-events";
import {
  getAppRuntime,
  type NativeSettingsWindowContext,
  type NativeSettingsWindowTarget
} from "./runtime";
import type {
  RemoteNotebookCatalogEntry,
  SyncConfigDocument
} from "./lib/sync-config";
import {
  confirmNativeMarkdownFileDelete,
  confirmNativeUnsavedMarkdownDocumentDiscard,
  downloadNativeWebImage,
  importNativeLocalFile,
  openNativeContainingFolder,
  openNativeMarkdownFile,
  openNativeMarkdownFileInNewWindow,
  openNativeMarkdownAttachment,
  openNativeLocalFiles,
  openNativeLocalImages,
  readNativeLocalImageFile,
  readNativeMarkdownFile,
  readNativeMarkdownTemplateFile,
  saveNativeClipboardAttachment,
  saveNativeHtmlFile,
  saveNativeMarkdownFile,
  saveNativePandocFile,
  saveNativePdfFile,
  showNativePandocSetup,
  writeNativeMarkdownTemplateFile,
  type NativeMarkdownDroppedTarget,
  type NativeMarkdownFolderFile,
  type NativeLocalFile,
  type NativePandocExportFormat
} from "./lib/tauri";
import {
  managedDocumentAbsolutePath,
  managedDocumentRelativePath
} from "./lib/settings/workspace-state";
import {
  createCustomMarkdownTemplateFromFile,
  loadMarkdownTemplatesFromEntries,
  markdownTemplateEntryFromTemplate,
  type MarkdownTemplate
} from "./lib/templates";
import {
  createImageDocumentTab,
  defaultSaveDirectoryFromFileTree,
  documentTabAsFolderFile,
  imageDocumentTabId,
  replaceTextRange,
  replaceTextRanges,
  restoreElementScrollTop,
  selectionAnchorsEqual,
  unsavedMarkdownFileNameFromTreeInput,
  type ImageDocumentTab
} from "./app/workspace-model";

const splitPaneKeyboardStepPercent = 5;
const selectionToolbarCopySuccessMs = 1600;
const sideDocumentPaneKeyboardStepPercent = 5;
const sideDocumentMainPanePercentMin = 35;
const sideDocumentMainPanePercentMax = 70;
const defaultSideDocumentMainPanePercent = 50;
const quietStatusOverlayInset = 56;

function nativeChildPath(parentPath: string, name: string) {
  const separator = parentPath.includes("\\") && !parentPath.includes("/") ? "\\" : "/";
  return `${parentPath.replace(/[\\/]+$/u, "")}${separator}${name}`;
}

function editorFileForNativeLocalFile(file: NativeLocalFile) {
  const editorFile = new File([], file.name, { type: "application/octet-stream" });
  Object.defineProperty(editorFile, "path", {
    configurable: false,
    enumerable: true,
    value: file.path
  });
  return editorFile;
}

function nativePathFromEditorFile(file: File) {
  const path = (file as File & { path?: unknown }).path;
  return typeof path === "string" && path ? path : null;
}

function persistSideDocumentGroup(group: StoredWorkspaceSideBySideGroup | null) {
  saveStoredWorkspaceState({ sideBySideGroup: group }).catch(() => {});
}

function nativeFileOperationFailureDescription(error: unknown) {
  return error instanceof Error
    ? error.message.trim()
    : typeof error === "string"
      ? error.trim()
      : "";
}

function nativeFileOperationFailureMessage(message: string, error: unknown) {
  const detail = nativeFileOperationFailureDescription(error);

  return detail ? `${message} ${detail}` : message;
}

export function clipboardImageSaveFailureDescription(error: unknown) {
  return nativeFileOperationFailureDescription(error);
}

export function clipboardImageSaveFailureMessage(message: string, error: unknown) {
  return nativeFileOperationFailureMessage(message, error);
}

export async function refreshImportedAttachmentTree(refreshTree: () => Promise<unknown>) {
  await refreshTree().catch(() => {});
}

type EditorMode = "source" | "split" | "visual";
type EditorSurface = "source" | "visual";
type DocumentTabViewState = {
  sourceScrollTop?: number;
  visualScrollTop?: number;
};
type PendingEditorModeScroll = {
  progress: number;
  tabId: string;
  targetSurface: EditorSurface;
};

export { runEditorLinkCommand } from "./app/editor-link-command";
export { globalSearchDebounceMs } from "./hooks/useWorkspaceSearch";

const SettingsWindow = lazy(async () => {
  const module = await import("./components/SettingsWindow");

  return { default: module.SettingsWindow };
});
const workspaceLinkIndexDeferMs = 320;
const runtimeErrorDiagnosticsToastId = "runtime-error-diagnostics";

export function shouldTriggerDevMockRuntimeError(search: string, dev = import.meta.env.DEV) {
  if (!dev) return false;

  return new URLSearchParams(search).get("mockError") === "1";
}

export default function App() {
  const isSettingsRoute = useSettingsWindowRoute();
  const runtime = getAppRuntime();
  const independentSettingsRouteSupported = runtime.features.settingsWindow
    || runtime.platform.resolveFormFactor() !== "mobile";

  return isSettingsRoute && independentSettingsRouteSupported ? <SettingsRouteApp /> : <WorkspaceApp />;
}

function runtimeErrorFromEvent(event: ErrorEvent | PromiseRejectionEvent) {
  if ("error" in event) return event.error ?? event.message;

  return event.reason;
}

function createRuntimeDiagnosticsReport(error: unknown, language: AppLanguage) {
  return generateCrashDiagnosticsReport({
    appVersion,
    componentStack: null,
    error,
    generatedAt: new Date(),
    language,
    osVersion: resolveDesktopOsVersion(),
    platform: resolveDesktopPlatform()
  });
}

function showRuntimeErrorDiagnosticsToast(error: unknown, language: AppLanguage) {
  showAppToast({
    action: {
      label: t(language, "app.errorToast.submitIssue"),
      onClick: () => {
        openNativeExternalUrl(
          generateDiagnosticsIssueUrl(createRuntimeDiagnosticsReport(error, language), {
            title: "Runtime error report"
          })
        ).catch(() => {
          showAppToast({
            id: `${runtimeErrorDiagnosticsToastId}-action`,
            message: t(language, "app.errorBoundary.issueFailed"),
            status: "error"
          });
        });
      }
    },
    description: t(language, "app.errorToast.description"),
    id: runtimeErrorDiagnosticsToastId,
    message: t(language, "app.errorToast.title"),
    status: "error",
    surface: "notice"
  });
}

function useRuntimeErrorDiagnostics(language: AppLanguage) {
  const mockErrorShownRef = useRef(false);

  useEffect(() => {
    const handleError = (event: ErrorEvent) => {
      showRuntimeErrorDiagnosticsToast(runtimeErrorFromEvent(event), language);
    };
    const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
      showRuntimeErrorDiagnosticsToast(runtimeErrorFromEvent(event), language);
    };

    window.addEventListener("error", handleError);
    window.addEventListener("unhandledrejection", handleUnhandledRejection);

    return () => {
      window.removeEventListener("error", handleError);
      window.removeEventListener("unhandledrejection", handleUnhandledRejection);
    };
  }, [language]);

  useEffect(() => {
    if (mockErrorShownRef.current || !shouldTriggerDevMockRuntimeError(window.location.search)) return;

    mockErrorShownRef.current = true;
    showRuntimeErrorDiagnosticsToast(new Error("Mock runtime error preview"), language);
  }, [language]);
}

function SettingsRouteApp() {
  const handleCloseSettings = useCallback(() => {
    hideSettingsWindow().catch(() => {});
  }, []);

  useSettingsWindowShortcut(handleCloseSettings);
  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => unknown) | null = null;

    listenNativeApplicationMenuCommands({ openSettings: handleCloseSettings }).then((stopListening) => {
      if (cancelled) {
        stopListening();
        return;
      }
      cleanup = stopListening;
    }).catch(() => {});

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [handleCloseSettings]);

  return (
    <Suspense fallback={<SettingsWindowLoadingShell onClose={handleCloseSettings} />}>
      <SettingsWindow />
    </Suspense>
  );
}

type SettingsModalRequest = {
  context: NativeSettingsWindowContext;
  generation: number;
  target?: NativeSettingsWindowTarget;
};

function WorkspaceApp() {
  const compactMode = useCompactMode();
  const editorWindowContext = useMemo(
    () => parseEditorWindowContext(window.location.search),
    []
  );
  const externalEditorWindow = isExternalEditorWindow(editorWindowContext);
  const primaryWindowOwner = compactMode.trueMobile || !externalEditorWindow;
  const workspacePersistencePolicy = primaryWindowOwner ? "shared" : "isolated";
  const primaryWorkspace = usePrimaryWorkspace({ trueMobile: compactMode.trueMobile });
  const fixedWorkspaceRoot = getAppRuntime().workspace.rootPolicy?.kind === "fixed";
  const canChooseLocalWorkspace = !fixedWorkspaceRoot && (
    compactMode.trueMobile || primaryWorkspace.canChooseDesktopRoot
  );
  const primaryRoot = primaryWorkspace.status === "ready" ? primaryWorkspace.root : null;
  const primaryIntegrationRoot = primaryWindowOwner ? primaryRoot : null;
  const onboardingVisible = primaryWindowOwner && (
    primaryWorkspace.status === "loading" ||
    primaryWorkspace.status === "needs-onboarding" ||
    primaryWorkspace.status === "recovery" ||
    primaryWorkspace.status === "error"
  );
  const desktopPlatform = resolveDesktopPlatform();
  const desktopOsVersion = resolveDesktopOsVersion();
  const webKitScrollWorkaround = webKitScrollWorkaroundForPlatform(desktopPlatform, desktopOsVersion);
  const nativeRuntimeAvailable = getAppRuntime().events.isAvailable();
  const appFeatures = getAppRuntime().features;
  const appFiles = getAppRuntime().files;
  const assetCleanupAvailable = Boolean(
    appFiles.listMarkdownReferenceFilesForPath && appFiles.trashMarkdownAssets
  );
  const mcpRuntime = getAppRuntime().mcp;
  const exportFeatureEnabled = appFeatures.export;
  const nativeWindowChromeEnabled = appFeatures.nativeWindowChrome && desktopPlatform !== "linux";
  const windowsSelfDrawnChromeEnabled = nativeWindowChromeEnabled && desktopPlatform === "windows";
  const pandocFeatureEnabled = appFeatures.pandoc;
  const updaterFeatureEnabled = appFeatures.updater;
  const appTheme = useAppTheme();
  const appLanguage = useAppLanguage();
  useRuntimeLogCapture();
  useRuntimeErrorDiagnostics(appLanguage.language);
  const editorPreferences = useEditorPreferences();
  const handleCompactPreferencesChange = useCallback((nextPreferencesOrUpdater: EditorPreferences | ((currentPreferences: EditorPreferences) => EditorPreferences)) => {
    const nextPreferences = typeof nextPreferencesOrUpdater === "function"
      ? nextPreferencesOrUpdater(editorPreferences.preferences)
      : nextPreferencesOrUpdater;
    editorPreferences.updatePreferences(nextPreferences);
    saveStoredEditorPreferences(nextPreferences)
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences.preferences, editorPreferences.updatePreferences]);
  const fileIgnoreSettings = useFileIgnoreSettings();
  const exportSettings = useExportSettings();
  const [markdownTemplates, setMarkdownTemplates] = useState<MarkdownTemplate[]>([]);
  const [settingsModalRequest, setSettingsModalRequest] = useState<SettingsModalRequest | null>(null);
  const settingsModalGenerationRef = useRef(0);
  const settingsModalOpenRef = useRef(false);
  const settingsModalReturnFocusRef = useRef<HTMLElement | null>(null);
  const openSettingsModal = useCallback(async (
    target?: NativeSettingsWindowTarget,
    projectRoot: string | null = null,
    workspaceSourcePath: string | null = null
  ) => {
    if (!settingsModalOpenRef.current) {
      settingsModalReturnFocusRef.current = globalThis.document.activeElement instanceof HTMLElement
        ? globalThis.document.activeElement
        : null;
    }
    settingsModalOpenRef.current = true;
    const generation = settingsModalGenerationRef.current + 1;
    settingsModalGenerationRef.current = generation;
    setSettingsModalRequest({
      context: {
        projectRoot,
        sourceWindowLabel: null,
        workspaceSourcePath
      },
      generation,
      target
    });

    try {
      const sourceWindowLabel = await getAppRuntime().window.getCurrentWindowLabel();
      setSettingsModalRequest((current) => current?.generation === generation
        ? {
            ...current,
            context: { ...current.context, sourceWindowLabel }
          }
        : current);
    } catch {
      // The modal can still operate without a native window label.
    }
  }, []);
  const closeSettingsModal = useCallback(() => {
    settingsModalOpenRef.current = false;
    const returnFocus = settingsModalReturnFocusRef.current;
    settingsModalReturnFocusRef.current = null;
    setSettingsModalRequest(null);
    window.requestAnimationFrame(() => {
      if (returnFocus?.isConnected) returnFocus.focus();
    });
  }, []);
  const [activeImageFile, setActiveImageFile] = useState<NativeMarkdownFolderFile | null>(null);
  const [imagePreviewObjectUrl, setImagePreviewObjectUrl] = useState<string | null>(null);
  const [imageTabs, setImageTabs] = useState<ImageDocumentTab[]>([]);
  const [activeTextSelection, setActiveTextSelection] = useState<EditorTextSelection | null>(null);
  const [selectedWordCount, setSelectedWordCount] = useState<number | null>(null);
  const [selectionToolbarAnchor, setSelectionToolbarAnchor] = useState<SelectionAnchor | null>(null);
  const [selectionToolbarActiveActions, setSelectionToolbarActiveActions] = useState<SelectionFormattingAction[]>([]);
  const [selectionToolbarHeadingLevel, setSelectionToolbarHeadingLevel] = useState<SelectionHeadingLevel | null>(null);
  const [selectionToolbarCopySucceeded, setSelectionToolbarCopySucceeded] = useState(false);
  const [activeOutlineIndex, setActiveOutlineIndex] = useState<number | null>(null);
  const setSelectionToolbarAnchorIfChanged = useCallback((nextAnchor: SelectionAnchor | null) => {
    setSelectionToolbarAnchor((currentAnchor) =>
      selectionAnchorsEqual(currentAnchor, nextAnchor) ? currentAnchor : nextAnchor
    );
  }, []);
  const [editorMode, setEditorMode] = useState<EditorMode>("visual");
  const [activeEditorSurface, setActiveEditorSurface] = useState<EditorSurface>("visual");
  const [readOnlyMode, setReadOnlyMode] = useState(false);
  const syncPathMutationRegistry = useMemo(() => new SyncPathMutationRegistry(), []);
  const [quickOpenOpen, setQuickOpenOpen] = useState(false);
  const [documentHistoryOpen, setDocumentHistoryOpen] = useState(false);
  const [documentHistoryRefreshKey, setDocumentHistoryRefreshKey] = useState(0);
  const [splitVisualPanePercent, setSplitVisualPanePercent] = useState(defaultSplitVisualPanePercent);
  const [sideDocumentMainPanePercent, setSideDocumentMainPanePercent] = useState(defaultSideDocumentMainPanePercent);
  const [editorTabDropTargetActive, setEditorTabDropTargetActive] = useState(false);
  const [visualEditorReadySequence, setVisualEditorReadySequence] = useState(0);
  const [exportSnapshot, setExportSnapshot] = useState<MarkdownExportSnapshot | null>(null);
  const sourceMode = editorMode === "source";
  const splitMode = editorMode === "split";
  const sourceSurfaceActive = sourceMode || (splitMode && activeEditorSurface === "source");
  const selectionToolbarCopySuccessTimerRef = useRef<number | null>(null);
  const exportRequestIdRef = useRef(0);
  const pendingSplitVisualPanePercentRef = useRef(defaultSplitVisualPanePercent);
  const pendingSideDocumentMainPanePercentRef = useRef(defaultSideDocumentMainPanePercent);
  const lastDocumentSearchRevealRevisionRef = useRef(0);
  const documentRevisionRef = useRef(0);
  const visualEditorReadyRevisionRef = useRef<number | null>(null);
  const visualEditorReadyDetailRef = useRef({
    chars: 0,
    path: null as string | null,
    sizeBytes: null as number | null
  });
  const largeMarkdownVisualBlockedRef = useRef(false);
  const mainDocumentPaneRef = useRef<HTMLDivElement | null>(null);
  const sourceScrollRef = useRef<HTMLElement | null>(null);
  const visualScrollRef = useRef<HTMLElement | null>(null);
  const mainVisualEditorsRef = useRef(new Map<string, EditorView>());
  const documentTabViewStatesRef = useRef(new Map<string, DocumentTabViewState>());
  const pendingEditorModeScrollRef = useRef<PendingEditorModeScroll | null>(null);
  const splitSurfaceRef = useRef<HTMLDivElement | null>(null);
  const sideDocumentSurfaceRef = useRef<HTMLDivElement | null>(null);
  const titlebarTabFocusTimerRef = useRef<number | null>(null);
  const splitScrollSyncTargetRef = useRef<EditorSurface | null>(null);
  const splitScrollSyncFrameRef = useRef<number | null>(null);
  const splitPaneResizeCleanupRef = useRef<(() => unknown) | null>(null);
  const sideDocumentPaneResizeCleanupRef = useRef<(() => unknown) | null>(null);
  // Late visual-editor updates may arrive after focus moves to source; source edits after that focus win.
  const sourceEditSequenceRef = useRef(0);
  const sourceFocusSourceEditSequenceRef = useRef(0);
  const syncingExternalDocumentHistoryRef = useRef(false);
  const exportContextRef = useRef({
    activeImageFile: false,
    content: "",
    hasOpenDocument: false,
    name: "Untitled.md",
    path: null as string | null
  });

  useEffect(() => {
    const root = globalThis.document?.documentElement;
    if (!root) return;

    if (webKitScrollWorkaround) {
      root.dataset.webkitScrollWorkaround = webKitScrollWorkaround;
      return () => {
        if (root.dataset.webkitScrollWorkaround === webKitScrollWorkaround) {
          delete root.dataset.webkitScrollWorkaround;
        }
      };
    }

    delete root.dataset.webkitScrollWorkaround;
  }, [webKitScrollWorkaround]);

  const translate = useCallback((key: I18nKey) => t(appLanguage.language, key), [appLanguage.language]);
  const clearSelectionToolbarCopySuccess = useCallback(() => {
    if (selectionToolbarCopySuccessTimerRef.current !== null) {
      window.clearTimeout(selectionToolbarCopySuccessTimerRef.current);
      selectionToolbarCopySuccessTimerRef.current = null;
    }

    setSelectionToolbarCopySucceeded(false);
  }, []);
  useEffect(() => {
    return () => {
      if (selectionToolbarCopySuccessTimerRef.current !== null) {
        window.clearTimeout(selectionToolbarCopySuccessTimerRef.current);
        selectionToolbarCopySuccessTimerRef.current = null;
      }
    };
  }, []);
  useEffect(() => {
    let cancelled = false;

    loadMarkdownTemplatesFromEntries(
      editorPreferences.preferences.markdownTemplates,
      readNativeMarkdownTemplateFile
    ).then((templates) => {
      if (!cancelled) setMarkdownTemplates(templates);
    }).catch(() => {
      if (!cancelled) setMarkdownTemplates([]);
    });

    return () => {
      cancelled = true;
    };
  }, [editorPreferences.preferences.markdownTemplates]);

  const editor = useCodeMirrorEditorController();
  const findEditorSearchMatches = editor.findSearchMatches;
  const getEditorCurrentMarkdown = editor.getCurrentMarkdown;
  const handleCodeMirrorEditorReady = editor.handleEditorReady;
  const insertEditorMarkdownImage = editor.insertMarkdownImage;
  const insertEditorMarkdownImages = editor.insertMarkdownImages;
  const insertEditorMarkdownImagesAtPoint = editor.insertMarkdownImagesAtPoint;
  const insertEditorMarkdownLink = editor.insertMarkdownLink;
  const insertEditorMarkdownLinks = editor.insertMarkdownLinks;
  const insertEditorMarkdownSnippet = editor.insertMarkdownSnippet;
  const insertEditorMarkdownTable = editor.insertMarkdownTable;
  const isEditorCurrentMarkdownEquivalent = editor.isCurrentMarkdownEquivalent;
  const replaceAllEditorSearchMatches = editor.replaceAllSearchMatches;
  const replaceEditorMarkdown = editor.replaceMarkdown;
  const replaceEditorSearchMatch = editor.replaceSearchMatch;
  const revealEditorSearchMatch = editor.revealSearchMatch;
  const runEditorShortcut = editor.runEditorShortcut;
  const runEditorSelectionFormattingAction = editor.runSelectionFormattingAction;
  const toggleEditorTaskList = editor.toggleTaskList;
  const getEditorSelectionAnchor = editor.getSelectionAnchor;
  const getEditorSelectionFormattingState = editor.getSelectionFormattingState;
  const hasEditorTextSelection = editor.hasTextSelection;
  const getMarkdownFromEditor = editor.getMarkdownFromEditor;
  const setEditorSelectionHeadingLevel = editor.setSelectionHeadingLevel;
  const showEditorSearchMatches = editor.showSearchMatches;
  const syncSelectionToolbarFormattingState = useCallback(() => {
    const formattingState = getEditorSelectionFormattingState();

    setSelectionToolbarActiveActions(formattingState.actions);
    setSelectionToolbarHeadingLevel(formattingState.headingLevel);
  }, [getEditorSelectionFormattingState]);
  const readCurrentMarkdownForDocument = useCallback((fallbackContent: string) => {
    if (sourceSurfaceActive || largeMarkdownVisualBlockedRef.current) return fallbackContent;

    return getEditorCurrentMarkdown(fallbackContent);
  }, [getEditorCurrentMarkdown, sourceSurfaceActive]);
  const isCurrentMarkdownEquivalentForDocument = useCallback((markdown: string) => {
    if (sourceSurfaceActive || largeMarkdownVisualBlockedRef.current) return undefined;

    return isEditorCurrentMarkdownEquivalent(markdown);
  }, [isEditorCurrentMarkdownEquivalent, sourceSurfaceActive]);
  const isDocumentEditorReady = useCallback(() => {
    if (sourceSurfaceActive || largeMarkdownVisualBlockedRef.current) return true;

    return visualEditorReadyRevisionRef.current === documentRevisionRef.current;
  }, [sourceSurfaceActive]);
  const handleVisualEditorReady = useCallback((...args: Parameters<typeof handleCodeMirrorEditorReady>) => {
    const [readyEditor] = args;
    handleCodeMirrorEditorReady(...args);
    if (readyEditor) {
      markAppPerformance("markdown-visual-ready", {
        ...visualEditorReadyDetailRef.current,
        revision: documentRevisionRef.current
      });
      visualEditorReadyRevisionRef.current = documentRevisionRef.current;
      setVisualEditorReadySequence((current) => current + 1);
    } else if (visualEditorReadyRevisionRef.current === documentRevisionRef.current) {
      visualEditorReadyRevisionRef.current = null;
    }
  }, [handleCodeMirrorEditorReady]);
  const handleActiveDiskFileContentChange = useCallback((change: ActiveDiskFileContentChange) => {
    if (largeMarkdownVisualBlockedRef.current) return false;

    syncingExternalDocumentHistoryRef.current = true;
    try {
      return replaceEditorMarkdown(change.content, {
        addToHistory: true,
        historyBaselineMarkdown: change.previousContent
      });
    } finally {
      syncingExternalDocumentHistoryRef.current = false;
    }
  }, [replaceEditorMarkdown]);
  const handleActiveOutlineIndexChange = useCallback((index: number | null) => {
    setActiveOutlineIndex((current) => current === index ? current : index);
  }, []);
  useDefaultContextMenuBlocker();
  const [viewportWidth, setViewportWidth] = useState(() => window.innerWidth);
  const fileTree = useMarkdownFileTree({
    globalIgnoreRules: fileIgnoreSettings.settings.rules,
    managedAttachmentFolder: editorPreferences.preferences.clipboardImageFolder,
    workspacePersistencePolicy
  });
  useEffect(() => {
    const handleViewportResize = () => {
      setViewportWidth(window.innerWidth);
    };

    window.addEventListener("resize", handleViewportResize);
    return () => window.removeEventListener("resize", handleViewportResize);
  }, []);
  const blankWorkspace = editorWindowContext.kind === "external-blank";
  const syncConfig = useSyncConfig();
  const {
    files: fileTreeFiles,
    createFile: createMarkdownTreeFileUnchecked,
    createFolder: createMarkdownTreeFolderUnchecked,
    clearProjectRoot,
    deleteFile: deleteMarkdownTreeFileUnchecked,
    fileTreeAssetsVisible,
    fileTreeSort,
    moveFile: moveMarkdownTreeFileUnchecked,
    open: fileTreeOpen,
    openFolderPath,
    renameFile: renameMarkdownTreeFileUnchecked,
    refresh: refreshMarkdownFileTree,
    refreshRevision: fileTreeRefreshRevision,
    resizing: fileTreeResizing,
    resize: resizeFileTree,
    endResize: endFileTreeResize,
    sourcePath: fileTreeSourcePath,
    rootNameForDocument,
    setRootFromMarkdownFilePath,
    setFileTreeSort,
    setFileTreeAssetsVisible,
    startResize: startFileTreeResize,
    toggle: toggleFileTree,
    width: fileTreeWidth,
    maxWidth: fileTreeMaxWidth,
    minWidth: fileTreeMinWidth,
    workspaceLayoutClassName,
    workspaceLayoutStyle
  } = fileTree;
  const editorImageSrcResolverForPath = useMemo(() => {
    const resolverByDocumentPath = new Map<string | null, (source: string) => string>();

    return (documentPath: string | null) => {
      const cachedResolver = resolverByDocumentPath.get(documentPath);
      if (cachedResolver) return cachedResolver;

      const fallbackResolver = createMarkdownImageSrcResolver(documentPath);
      const resolver = (source: string) => (
        documentPath
          ? appFiles.resolveMarkdownImageSrc?.(documentPath, source)
          : undefined
      ) ?? fallbackResolver(source);
      resolverByDocumentPath.set(documentPath, resolver);
      return resolver;
    };
  }, [appFiles.resolveMarkdownImageSrc, fileTreeFiles, fileTreeRefreshRevision]);
  const activeDocumentPathRef = useRef<string | null>(null);
  const currentPrimaryRootRef = useRef(primaryIntegrationRoot);
  currentPrimaryRootRef.current = primaryIntegrationRoot;
  const handlePrimaryFilesChanged = useCallback((root: string) => {
    if (currentPrimaryRootRef.current !== root) return;
    return refreshMarkdownFileTree(activeDocumentPathRef.current);
  }, [refreshMarkdownFileTree]);
  const appSync = useAppSyncCoordinator({
    configDocument: syncConfig.appliedDocument,
    dejavuSyncAvailable: Boolean(appFeatures.dejavuSync),
    onFilesChanged: handlePrimaryFilesChanged,
    primaryRoot: primaryIntegrationRoot,
    reloadConfig: syncConfig.reload,
    translate
  });
  useSyncConflictHistory({
    available: Boolean(appFeatures.dejavuSync),
    notesRoot: primaryIntegrationRoot,
    translate
  });
  const defaultMarkdownSaveDirectory = useMemo(
    () => defaultSaveDirectoryFromFileTree(fileTreeSourcePath),
    [fileTreeSourcePath]
  );
  const saveAsWorkspacePolicy = useMemo(() => (
    primaryWindowOwner && !compactMode.trueMobile
      ? { kind: "primary" as const, root: primaryRoot }
      : { kind: "standalone" as const }
  ), [compactMode.trueMobile, primaryRoot, primaryWindowOwner]);
  const confirmDiscardUnsavedChanges = useCallback((currentDocument: { name: string }) => {
    return confirmNativeUnsavedMarkdownDocumentDiscard(currentDocument.name, {
      cancelLabel: translate("app.cancelDiscardUnsavedMarkdownDocument"),
      message: translate("app.confirmDiscardUnsavedMarkdownDocument"),
      okLabel: translate("app.confirmDiscardUnsavedMarkdownDocumentAction")
    });
  }, [translate]);
  const runApplicationSyncNow = useCallback(
    () => appSync.run("manual"),
    [appSync.run]
  );
  const sidebarSyncState: SidebarSyncButtonState = appSync.running
    ? "running"
    : appSync.status?.completionState === "failed"
      ? "failed"
      : appSync.status?.completionState === "succeeded"
        ? "succeeded"
        : !primaryIntegrationRoot || syncConfig.appliedDocument?.readiness !== "ready"
          ? "unavailable"
          : "idle";
  const sidebarSyncAvailable = primaryWindowOwner &&
    !compactMode.trueMobile &&
    appFeatures.projectSync;
  const compactSyncSettings = useCompactSyncSettings({
    available: appFeatures.projectSync,
    observedLoadResult: syncConfig.loadResult,
    primaryRoot: primaryIntegrationRoot,
    runImmediate: runApplicationSyncNow,
    shouldBegin: false
  });
  const switchDesktopNotebookRef = useRef<(
    path?: string
  ) => Promise<string | null>>(async () => null);
  const handleNativeNotebookDirectory = useCallback((path: string) => {
    if (!canChooseLocalWorkspace) return null;
    if (primaryWindowOwner) return switchDesktopNotebookRef.current(path);
    return requestPrimaryNotebookSwitch({ path, source: "native-open" });
  }, [canChooseLocalWorkspace, primaryWindowOwner]);
  const syncStatusLabel = useMemo(() => {
    if (!appSync.status) return null;
    const completionLabel = appSync.status.completionState === "attempting"
      ? translate("settings.sync.status.attempting")
      : appSync.status.completionState === "failed"
        ? translate("settings.sync.status.failed")
        : translate("settings.sync.status.succeeded");
    return `${translate("settings.sync.lastSync")} ${completionLabel}`;
  }, [appSync.status, translate]);
  const markdownDocument = useMarkdownDocument({
    autoSaveEnabled: !compactMode.compact && editorPreferences.preferences.autoSaveEnabled,
    autoSaveIntervalMinutes: editorPreferences.preferences.autoSaveIntervalMinutes,
    confirmDiscardUnsavedChanges,
    defaultSaveDirectory: defaultMarkdownSaveDirectory,
    documentTabsEnabled: editorPreferences.preferences.showDocumentTabs,
    editorReady: isDocumentEditorReady,
    getCurrentMarkdown: readCurrentMarkdownForDocument,
    globalIgnoreRules: fileIgnoreSettings.settings.rules,
    isCurrentMarkdownEquivalent: isCurrentMarkdownEquivalentForDocument,
    managedWorkspace: compactMode.trueMobile,
    nativeCloseBlocked: settingsModalRequest !== null,
    nativeOpenPolicy: compactMode.trueMobile
      ? "managed"
      : primaryWindowOwner
        ? "spawn-external"
        : "editor",
    onActiveDiskFileContentChange: handleActiveDiskFileContentChange,
    onMarkdownTreeChange: refreshMarkdownFileTree,
    onTreeRootFromFolderPath: openFolderPath,
    onTreeRootFromFilePath: setRootFromMarkdownFilePath,
    onSwitchNotebookDirectory: handleNativeNotebookDirectory,
    openDroppedFilesInTabs: editorPreferences.preferences.openDroppedFilesInTabs,
    preferencesReady: !editorPreferences.loading && !compactMode.trueMobile && (
      !primaryWindowOwner ||
      primaryWorkspace.status === "ready" ||
      primaryWorkspace.status === "deferred"
    ),
    restoreWorkspaceOnStartup:
      primaryWindowOwner &&
      primaryWorkspace.status === "ready" &&
      editorPreferences.preferences.restoreWorkspaceOnStartup,
    restoreWorkspaceRoot: primaryIntegrationRoot,
    saveAsWorkspacePolicy,
    windowContext: editorWindowContext,
    workspaceSourcePath: fileTreeSourcePath,
    workspacePersistencePolicy
  });
  const {
    clearRecentMarkdownFiles,
    clearOpenDocument,
    createBlankDocument,
    confirmCanDiscardCurrentDocument,
    detachDeletedDocumentFile,
    document,
    tabs: documentTabs,
    activeTabId,
    closeMarkdownTab,
    getDirtyMarkdownFileContent,
    handleDroppedMarkdownPath,
    handleMarkdownChange,
    handleMarkdownTabChange,
    openMarkdownFile,
    openRecentMarkdownFile,
    persistManagedDocumentPath,
    openTreeMarkdownFileInBackground,
    openTreeMarkdownFile,
    outlineItems,
    replaceOpenDocumentFile,
    replaceMovedOpenDocumentFile,
    requestAppExit,
    recentFiles: recentMarkdownFiles,
    rememberMarkdownTabVisualBaseline,
    restoreDocumentContent,
    saveCurrentDocumentContent,
    saveCurrentDocument,
    saveDirtyMarkdownFiles,
    saveDirtyMarkdownPaths,
    saveMarkdownTab,
    selectMarkdownTab,
    wordCount
  } = markdownDocument;
  const { guardedPaths } = useSyncPathGuard({
    enabled: primaryWindowOwner,
    mutationRegistry: syncPathMutationRegistry,
    notesRoot: primaryIntegrationRoot,
    saveDirtyMarkdownPaths
  });
  const mainEditorReadOnly = editorReadOnlyForPath(readOnlyMode, document.path, guardedPaths);
  const showSyncPathGuardedNotice = useCallback(() => {
    showAppToast({
      message: translate("app.syncPathGuarded"),
      status: "success",
      surface: "notice"
    });
  }, [translate]);
  const guardFileMutation = useCallback((mutation: {
    destinationPath?: string | null;
    sourcePath?: string | null;
  }) => {
    if (
      !syncMutationIntersectsGuardedPaths(mutation, guardedPaths) &&
      !syncPathMutationRegistry.isBlocked(mutation)
    ) {
      return false;
    }
    showSyncPathGuardedNotice();
    return true;
  }, [guardedPaths, showSyncPathGuardedNotice, syncPathMutationRegistry]);
  const createMarkdownTreeFile = useCallback((
    fileName: string,
    parentPath: string | null = null,
    contents?: string
  ) => {
    const parent = parentPath ?? fileTreeSourcePath;
    const mutation = { destinationPath: parent ? nativeChildPath(parent, fileName) : null };
    if (parent && guardFileMutation(mutation)) {
      return Promise.resolve(null);
    }
    const lease = parent ? syncPathMutationRegistry.acquire(mutation) : null;
    if (parent && !lease) {
      guardFileMutation(mutation);
      return Promise.resolve(null);
    }
    return createMarkdownTreeFileUnchecked(fileName, parentPath, contents)
      .finally(() => lease?.release());
  }, [createMarkdownTreeFileUnchecked, fileTreeSourcePath, guardFileMutation, syncPathMutationRegistry]);
  const createMarkdownTreeFolder = useCallback((
    folderName: string,
    parentPath: string | null = null
  ) => {
    const parent = parentPath ?? fileTreeSourcePath;
    const mutation = { destinationPath: parent ? nativeChildPath(parent, folderName) : null };
    if (parent && guardFileMutation(mutation)) {
      return Promise.resolve(null);
    }
    const lease = parent ? syncPathMutationRegistry.acquire(mutation) : null;
    if (parent && !lease) {
      guardFileMutation(mutation);
      return Promise.resolve(null);
    }
    return createMarkdownTreeFolderUnchecked(folderName, parentPath)
      .finally(() => lease?.release());
  }, [createMarkdownTreeFolderUnchecked, fileTreeSourcePath, guardFileMutation, syncPathMutationRegistry]);
  const renameMarkdownTreeFile = useCallback((file: NativeMarkdownFolderFile, fileName: string) => {
    const destinationPath = nativeChildPath(
      file.path.replace(/[\\/][^\\/]+$/u, ""),
      fileName
    );
    const mutation = { destinationPath, sourcePath: file.path };
    if (guardFileMutation(mutation)) return Promise.resolve(null);
    const lease = syncPathMutationRegistry.acquire(mutation);
    if (!lease) {
      guardFileMutation(mutation);
      return Promise.resolve(null);
    }
    return renameMarkdownTreeFileUnchecked(file, fileName)
      .finally(() => lease.release());
  }, [guardFileMutation, renameMarkdownTreeFileUnchecked, syncPathMutationRegistry]);
  const deleteMarkdownTreeFile = useCallback((file: NativeMarkdownFolderFile) => {
    const mutation = { sourcePath: file.path };
    if (guardFileMutation(mutation)) return Promise.resolve(false);
    const lease = syncPathMutationRegistry.acquire(mutation);
    if (!lease) {
      guardFileMutation(mutation);
      return Promise.resolve(false);
    }
    return deleteMarkdownTreeFileUnchecked(file)
      .finally(() => lease.release());
  }, [deleteMarkdownTreeFileUnchecked, guardFileMutation, syncPathMutationRegistry]);
  const getDirtyAssetCleanupDocuments = useCallback(
    () => documentTabs.flatMap((tab) => {
      if (!tab.path || !tab.dirty) return [];

      return [{
        content: getDirtyMarkdownFileContent(tab.path) ?? tab.content,
        path: tab.path
      }];
    }),
    [documentTabs, getDirtyMarkdownFileContent]
  );
  const assetCleanup = useWorkspaceAssetCleanup({
    getDirtyDocuments: getDirtyAssetCleanupDocuments,
    globalIgnoreRules: fileIgnoreSettings.settings.rules,
    managedFolder: editorPreferences.preferences.clipboardImageFolder,
    onTreeRefresh: refreshMarkdownFileTree,
    rootPath: fileTreeSourcePath
  });
  const [notebookRestoreConfigDocument, setNotebookRestoreConfigDocument] = useState<SyncConfigDocument | null>(null);
  const notebookSwitch = useNotebookSwitchCoordinator({
    appSync,
    configDocument: notebookRestoreConfigDocument ?? syncConfig.appliedDocument,
    flushActiveDocument: saveDirtyMarkdownFiles,
    primaryRoot: primaryIntegrationRoot,
    primaryWindowOwner,
    primaryWorkspace,
    trueMobile: compactMode.trueMobile
  });
  switchDesktopNotebookRef.current = notebookSwitch.switchDesktopNotebook;
  const [remoteNotebookDialogOpen, setRemoteNotebookDialogOpen] = useState(false);
  const [remoteNotebookEntries, setRemoteNotebookEntries] = useState<RemoteNotebookCatalogEntry[]>([]);
  const [remoteNotebookError, setRemoteNotebookError] = useState<string | null>(null);
  const [remoteNotebookLoading, setRemoteNotebookLoading] = useState(false);
  const [remoteNotebookCatalogLoaded, setRemoteNotebookCatalogLoaded] = useState(false);
  const [remoteNotebookCatalogMode, setRemoteNotebookCatalogMode] = useState<"browse" | "select">("browse");
  const [remoteNotebookPendingRevision, setRemoteNotebookPendingRevision] = useState<string | null>(null);
  const [remoteNotebookCatalogRevision, setRemoteNotebookCatalogRevision] = useState<string | null>(null);
  const [mobileNotebookDialogOpen, setMobileNotebookDialogOpen] = useState(false);
  const [mobileNotebookLocalNames, setMobileNotebookLocalNames] = useState<string[]>([]);
  const [compactNavigationRequest, setCompactNavigationRequest] = useState<{
    id: number;
    page: { kind: "sync-status" };
    retainUntilEditor: boolean;
  } | null>(null);
  const remoteNotebookRequestGenerationRef = useRef(0);
  const compactNavigationRequestIdRef = useRef(0);
  const currentDesktopNotebookName = primaryWindowOwner &&
    !compactMode.trueMobile &&
    primaryWorkspace.status === "ready"
    ? pathNameFromPath(primaryWorkspace.root)
    : null;
  const handlePrimaryCloudNotebookRestoreRequest = useCallback(async (
    request: PrimaryCloudNotebookRestoreRequest
  ) => {
    if (
      compactMode.trueMobile ||
      !primaryWindowOwner ||
      remoteNotebookDialogOpen ||
      syncConfig.appliedDocument?.revision !== request.revision
    ) return false;

    if (request.provider === "s3") {
      if (!appFeatures.dejavuSync) return false;
      if (primaryWorkspace.status !== "ready" || primaryWorkspace.root !== request.notesRoot) {
        return false;
      }
      await getAppRuntime().syncConfig.bindRepository({
        displayName: request.displayName,
        notesRoot: request.notesRoot,
        repositoryId: request.repositoryId
      });
      return true;
    }

    if (request.remoteName === currentDesktopNotebookName) {
      return Boolean(await appSync.run("manual", request.revision));
    }

    const restoredRoot = await notebookSwitch.restoreDesktopNotebook(
      request.remoteName,
      primaryWorkspace.workspaceRoot ?? undefined
    );
    return restoredRoot !== null;
  }, [
    appSync.run,
    appFeatures.dejavuSync,
    compactMode.trueMobile,
    currentDesktopNotebookName,
    notebookSwitch.restoreDesktopNotebook,
    primaryWindowOwner,
    primaryWorkspace.root,
    primaryWorkspace.status,
    primaryWorkspace.workspaceRoot,
    remoteNotebookDialogOpen,
    syncConfig.appliedDocument?.revision
  ]);
  const handlePrimaryCloudNotebookRestoreRequestRef = useRef(
    handlePrimaryCloudNotebookRestoreRequest
  );
  handlePrimaryCloudNotebookRestoreRequestRef.current =
    handlePrimaryCloudNotebookRestoreRequest;
  const primaryCloudNotebookRestoreRegistrationRef = useRef<Promise<unknown>>(
    Promise.resolve()
  );
  useEffect(() => {
    if (!primaryWindowOwner || compactMode.trueMobile) return;

    let active = true;
    let deactivate!: () => undefined;
    const deactivated = new Promise<undefined>((resolve) => {
      deactivate = () => {
        resolve(undefined);
        return undefined;
      };
    });
    const registration = primaryCloudNotebookRestoreRegistrationRef.current.then(async () => {
      if (!active) return;

      const cleanup = await listenPrimaryCloudNotebookRestoreRequested((request) => {
        if (!active) return false;
        return handlePrimaryCloudNotebookRestoreRequestRef.current(request);
      });
      if (!active) {
        await cleanup();
        return;
      }

      await deactivated;
      await cleanup();
    });
    primaryCloudNotebookRestoreRegistrationRef.current = registration.catch(() => undefined);

    return () => {
      active = false;
      deactivate();
    };
  }, [
    compactMode.trueMobile,
    primaryWindowOwner
  ]);
  useEffect(() => () => {
    remoteNotebookRequestGenerationRef.current += 1;
  }, []);
  const closeRemoteNotebookDialog = useCallback(() => {
    remoteNotebookRequestGenerationRef.current += 1;
    setRemoteNotebookDialogOpen(false);
    setRemoteNotebookLoading(false);
    setRemoteNotebookCatalogLoaded(false);
    setRemoteNotebookCatalogMode("browse");
    setRemoteNotebookPendingRevision(null);
    setRemoteNotebookCatalogRevision(null);
    setRemoteNotebookEntries([]);
    setNotebookRestoreConfigDocument(null);
  }, []);
  const loadRemoteNotebookCatalog = useCallback(async (revision: string) => {
    const requestGeneration = remoteNotebookRequestGenerationRef.current + 1;
    remoteNotebookRequestGenerationRef.current = requestGeneration;
    setRemoteNotebookPendingRevision(null);
    setRemoteNotebookCatalogRevision(revision);
    setRemoteNotebookLoading(true);
    setRemoteNotebookCatalogLoaded(false);
    setRemoteNotebookError(null);
    try {
      const entries = await getAppRuntime().syncConfig.listNotebooks({ revision });
      if (remoteNotebookRequestGenerationRef.current !== requestGeneration) return;
      setRemoteNotebookEntries(entries.filter((entry) => (
        entry.provider !== "s3" || appFeatures.dejavuSync
      )));
      setRemoteNotebookCatalogLoaded(true);
    } catch {
      if (remoteNotebookRequestGenerationRef.current !== requestGeneration) return;
      setRemoteNotebookEntries([]);
      setRemoteNotebookError(translate("notebooks.remote.refreshError"));
    } finally {
      if (remoteNotebookRequestGenerationRef.current === requestGeneration) {
        setRemoteNotebookLoading(false);
      }
    }
  }, [appFeatures.dejavuSync, translate]);
  const openDesktopRemoteNotebookDialog = useCallback(async ({
    requireEstablishedWorkspace = false
  }: {
    requireEstablishedWorkspace?: boolean;
  } = {}) => {
    if (compactMode.trueMobile || !canChooseLocalWorkspace) return;
    if (
      requireEstablishedWorkspace && (
        primaryWorkspace.status !== "ready" ||
        !primaryWorkspace.root ||
        !primaryWorkspace.workspaceRoot
      )
    ) return;
    const currentResult = syncConfig.status === "loaded"
      ? syncConfig.loadResult
      : await syncConfig.reload();
    if (
      currentResult?.status !== "loaded" ||
      !currentResult.configured ||
      !currentResult.revision
    ) {
      await openSettingsModal("sync", null, null);
      return;
    }

    setRemoteNotebookDialogOpen(true);
    setRemoteNotebookEntries([]);
    setRemoteNotebookError(null);
    setRemoteNotebookCatalogLoaded(false);
    setRemoteNotebookCatalogMode(requireEstablishedWorkspace ? "select" : "browse");
    setRemoteNotebookCatalogRevision(currentResult.revision);
    setNotebookRestoreConfigDocument({
      config: currentResult.config,
      configured: currentResult.configured,
      issues: currentResult.issues,
      readiness: currentResult.readiness,
      revision: currentResult.revision
    });
    if (syncConfig.appliedDocument?.revision === currentResult.revision) {
      await loadRemoteNotebookCatalog(currentResult.revision);
      return;
    }
    setRemoteNotebookLoading(true);
    setRemoteNotebookPendingRevision(currentResult.revision);
  }, [
    canChooseLocalWorkspace,
    compactMode.trueMobile,
    loadRemoteNotebookCatalog,
    openSettingsModal,
    primaryWorkspace.root,
    primaryWorkspace.status,
    primaryWorkspace.workspaceRoot,
    syncConfig.appliedDocument?.revision,
    syncConfig.loadResult,
    syncConfig.reload,
    syncConfig.status
  ]);
  useEffect(() => {
    if (
      !remoteNotebookDialogOpen ||
      !remoteNotebookPendingRevision ||
      syncConfig.appliedDocument?.revision !== remoteNotebookPendingRevision
    ) return;
    loadRemoteNotebookCatalog(remoteNotebookPendingRevision).catch(() => {});
  }, [
    loadRemoteNotebookCatalog,
    remoteNotebookDialogOpen,
    remoteNotebookPendingRevision,
    syncConfig.appliedDocument?.revision
  ]);
  useEffect(() => {
    if (
      !remoteNotebookDialogOpen ||
      !remoteNotebookCatalogLoaded ||
      remoteNotebookLoading ||
      remoteNotebookError ||
      remoteNotebookEntries.length > 0 ||
      remoteNotebookCatalogMode !== "select" ||
      !notebookRestoreConfigDocument?.config.enabled ||
      notebookRestoreConfigDocument.readiness !== "ready" ||
      !remoteNotebookCatalogRevision
    ) return;
    const revision = remoteNotebookCatalogRevision;
    closeRemoteNotebookDialog();
    appSync.run("manual", revision).catch(() => null);
  }, [
    appSync.run,
    closeRemoteNotebookDialog,
    remoteNotebookCatalogLoaded,
    remoteNotebookCatalogMode,
    remoteNotebookCatalogRevision,
    remoteNotebookDialogOpen,
    remoteNotebookEntries.length,
    remoteNotebookError,
    remoteNotebookLoading,
    notebookRestoreConfigDocument
  ]);
  const refreshDesktopRemoteNotebookCatalog = useCallback(async () => {
    const revision = syncConfig.appliedDocument?.revision;
    if (!revision) {
      closeRemoteNotebookDialog();
      await openSettingsModal("sync", null, null);
      return;
    }
    await loadRemoteNotebookCatalog(revision);
  }, [closeRemoteNotebookDialog, loadRemoteNotebookCatalog, openSettingsModal, syncConfig.appliedDocument?.revision]);
  const requireCurrentRemoteNotebookCatalogRevision = useCallback(() => {
    const configuredRevision = syncConfig.status === "loaded" && syncConfig.loadResult?.status === "loaded"
      ? syncConfig.loadResult.revision
      : null;
    if (!remoteNotebookCatalogRevision || configuredRevision !== remoteNotebookCatalogRevision) {
      closeRemoteNotebookDialog();
      throw new Error("The cloud notebook catalog is stale.");
    }
    return remoteNotebookCatalogRevision;
  }, [
    closeRemoteNotebookDialog,
    remoteNotebookCatalogRevision,
    syncConfig.loadResult,
    syncConfig.status
  ]);
  const restoreDesktopRemoteNotebook = useCallback(async (entry: RemoteNotebookCatalogEntry) => {
    requireCurrentRemoteNotebookCatalogRevision();
    if (entry.provider === "s3") {
      if (!appFeatures.dejavuSync) throw new Error("S3 repository binding is unavailable.");
      if (primaryWorkspace.status !== "ready" || !primaryWorkspace.root) {
        throw new Error("Choose a local notebook directory before binding an S3 repository.");
      }
      await getAppRuntime().syncConfig.bindRepository({
        displayName: entry.displayName,
        notesRoot: primaryWorkspace.root,
        repositoryId: entry.repositoryId
      });
      closeRemoteNotebookDialog();
      return;
    }
    const restoredRoot = await notebookSwitch.restoreDesktopNotebook(
      entry.name,
      primaryWorkspace.workspaceRoot ?? undefined
    );
    if (!restoredRoot) throw new Error("Notebook restore did not complete.");
    closeRemoteNotebookDialog();
  }, [
    appFeatures.dejavuSync,
    closeRemoteNotebookDialog,
    notebookSwitch.restoreDesktopNotebook,
    primaryWorkspace.root,
    primaryWorkspace.status,
    primaryWorkspace.workspaceRoot,
    requireCurrentRemoteNotebookCatalogRevision
  ]);
  const selectDesktopRemoteNotebook = useCallback(async (entry: RemoteNotebookCatalogEntry) => {
    if (
      entry.provider === "s3"
      || remoteNotebookCatalogMode !== "select"
      || entry.name !== currentDesktopNotebookName
    ) {
      await restoreDesktopRemoteNotebook(entry);
      return;
    }
    const revision = requireCurrentRemoteNotebookCatalogRevision();
    const result = await appSync.run("manual", revision);
    if (!result) throw new Error("Notebook synchronization did not complete.");
    closeRemoteNotebookDialog();
  }, [
    appSync.run,
    closeRemoteNotebookDialog,
    currentDesktopNotebookName,
    remoteNotebookCatalogMode,
    requireCurrentRemoteNotebookCatalogRevision,
    restoreDesktopRemoteNotebook
  ]);
  const closeMobileNotebookDialog = useCallback(() => {
    remoteNotebookRequestGenerationRef.current += 1;
    setMobileNotebookDialogOpen(false);
    setRemoteNotebookLoading(false);
    setRemoteNotebookPendingRevision(null);
    setRemoteNotebookCatalogRevision(null);
    setRemoteNotebookEntries([]);
    setNotebookRestoreConfigDocument(null);
  }, []);
  const openMobileSyncSettings = useCallback(() => {
    closeMobileNotebookDialog();
    compactNavigationRequestIdRef.current += 1;
    setCompactNavigationRequest({
      id: compactNavigationRequestIdRef.current,
      page: { kind: "sync-status" },
      retainUntilEditor: onboardingVisible
    });
  }, [closeMobileNotebookDialog, onboardingVisible]);
  const completeCompactNavigationRequest = useCallback((requestId: number) => {
    setCompactNavigationRequest((request) => request?.id === requestId ? null : request);
  }, []);
  const configuredRemoteNotebookRevision = syncConfig.status === "loaded" && syncConfig.loadResult?.status === "loaded"
    ? syncConfig.loadResult.revision
    : null;
  useEffect(() => {
    if (!remoteNotebookCatalogRevision) return;
    const appliedRevision = syncConfig.appliedDocument?.revision ?? null;
    if (
      configuredRemoteNotebookRevision === remoteNotebookCatalogRevision &&
      (!appliedRevision || appliedRevision === remoteNotebookCatalogRevision)
    ) return;
    if (remoteNotebookDialogOpen) closeRemoteNotebookDialog();
    if (mobileNotebookDialogOpen) closeMobileNotebookDialog();
  }, [
    closeMobileNotebookDialog,
    closeRemoteNotebookDialog,
    configuredRemoteNotebookRevision,
    mobileNotebookDialogOpen,
    remoteNotebookCatalogRevision,
    remoteNotebookDialogOpen,
    syncConfig.appliedDocument?.revision
  ]);
  const openMobileNotebookDialog = useCallback(async () => {
    if (!compactMode.trueMobile || !canChooseLocalWorkspace) return;
    const requestGeneration = remoteNotebookRequestGenerationRef.current + 1;
    remoteNotebookRequestGenerationRef.current = requestGeneration;
    setMobileNotebookDialogOpen(true);
    setMobileNotebookLocalNames([]);
    setRemoteNotebookEntries([]);
    setRemoteNotebookError(null);
    setRemoteNotebookLoading(true);
    try {
      const listLocalNames = getAppRuntime().workspace.listManagedNotebookNames;
      const localNamesPromise = listLocalNames ? listLocalNames() : Promise.resolve([]);
      const configResultPromise = syncConfig.status === "loaded"
        ? Promise.resolve(syncConfig.loadResult)
        : syncConfig.reload();
      const [localNames, currentResult] = await Promise.all([
        localNamesPromise,
        configResultPromise
      ]);
      if (remoteNotebookRequestGenerationRef.current !== requestGeneration) return;
      setMobileNotebookLocalNames(localNames);
      if (
        currentResult?.status !== "loaded" ||
        !currentResult.configured ||
        !currentResult.revision
      ) {
        return;
      }
      setNotebookRestoreConfigDocument({
        config: currentResult.config,
        configured: currentResult.configured,
        issues: currentResult.issues,
        readiness: currentResult.readiness,
        revision: currentResult.revision
      });
      setRemoteNotebookCatalogRevision(currentResult.revision);
      const entries = await getAppRuntime().syncConfig.listNotebooks({
        revision: currentResult.revision
      });
      if (remoteNotebookRequestGenerationRef.current !== requestGeneration) return;
      setRemoteNotebookEntries(entries.filter((entry) => (
        entry.provider !== "s3" || appFeatures.dejavuSync
      )));
    } catch {
      if (remoteNotebookRequestGenerationRef.current !== requestGeneration) return;
      setRemoteNotebookError(translate("notebooks.remote.refreshError"));
    } finally {
      if (remoteNotebookRequestGenerationRef.current === requestGeneration) {
        setRemoteNotebookLoading(false);
      }
    }
  }, [
    appFeatures.dejavuSync,
    canChooseLocalWorkspace,
    compactMode.trueMobile,
    syncConfig.loadResult,
    syncConfig.reload,
    syncConfig.status,
    translate
  ]);
  const switchMobileNotebook = useCallback(async (name: string) => {
    const switchedRoot = await notebookSwitch.switchManagedNotebook(name);
    if (!switchedRoot) throw new Error("Notebook switch did not complete.");
    closeMobileNotebookDialog();
  }, [closeMobileNotebookDialog, notebookSwitch.switchManagedNotebook]);
  const restoreMobileNotebook = useCallback(async (entry: RemoteNotebookCatalogEntry) => {
    const configuredRevision = syncConfig.status === "loaded" && syncConfig.loadResult?.status === "loaded"
      ? syncConfig.loadResult.revision
      : null;
    if (!remoteNotebookCatalogRevision || configuredRevision !== remoteNotebookCatalogRevision) {
      closeMobileNotebookDialog();
      throw new Error("The cloud notebook catalog is stale.");
    }
    if (entry.provider === "s3") {
      if (!appFeatures.dejavuSync) throw new Error("S3 repository binding is unavailable.");
      if (primaryWorkspace.status !== "ready" || !primaryWorkspace.root) {
        throw new Error("A local notebook directory is required.");
      }
      await getAppRuntime().syncConfig.bindRepository({
        displayName: entry.displayName,
        notesRoot: primaryWorkspace.root,
        repositoryId: entry.repositoryId
      });
      closeMobileNotebookDialog();
      return;
    }
    const restoredRoot = await notebookSwitch.restoreManagedNotebook(entry.name);
    if (!restoredRoot) throw new Error("Notebook restore did not complete.");
    closeMobileNotebookDialog();
  }, [
    appFeatures.dejavuSync,
    closeMobileNotebookDialog,
    notebookSwitch.restoreManagedNotebook,
    primaryWorkspace.root,
    primaryWorkspace.status,
    remoteNotebookCatalogRevision,
    syncConfig.loadResult,
    syncConfig.status
  ]);
  activeDocumentPathRef.current = document.path;
  useWorkspaceResourceSnapshotResponder({
    documentTabs,
    workspaceSourcePath: blankWorkspace ? null : fileTreeSourcePath
  });
  const compactAutoSaveErrorMessages = useMemo(() => ({
    noSpace: translate("compact.save.errorNoSpace"),
    permission: translate("compact.save.errorPermission"),
    readOnly: translate("compact.save.errorReadOnly")
  }), [translate]);
  const compactSaveState = useCompactAutoSave({
    content: document.content,
    dirty: document.dirty,
    documentKey: activeTabId,
    enabled: compactMode.compact,
    errorMessage: translate("compact.save.error"),
    errorMessages: compactAutoSaveErrorMessages,
    saveDirtyMarkdownFiles
  });
  const openManagedTreeMarkdownFile = useCallback(
    (file: Parameters<typeof openTreeMarkdownFile>[0]) => openTreeMarkdownFile(file, { managed: true }),
    [openTreeMarkdownFile]
  );
  const managedDocumentRef = useRef(document);
  const clearOpenDocumentRef = useRef(clearOpenDocument);
  managedDocumentRef.current = document;
  clearOpenDocumentRef.current = clearOpenDocument;
  const clearManagedDocument = useCallback(() => {
    const currentDocument = managedDocumentRef.current;
    if (
      !currentDocument.open ||
      currentDocument.path !== null ||
      currentDocument.name !== "Untitled.md" ||
      currentDocument.content !== "" ||
      currentDocument.dirty ||
      currentDocument.revision !== 0
    ) {
      return false;
    }

    return clearOpenDocumentRef.current({ persistWorkspace: false });
  }, []);
  const primaryTreeGenerationRef = useRef(0);
  const primaryWorkspaceReadyRef = useRef(primaryWorkspace.status === "ready");
  const primaryWorkspaceDeferredCleanupPendingRef = useRef(false);
  const openPrimaryFolderPathRef = useRef(openFolderPath);
  openPrimaryFolderPathRef.current = openFolderPath;
  const openPrimaryFolderPathEffectDependency = fixedWorkspaceRoot ? null : openFolderPath;
  useEffect(() => {
    if (!primaryWindowOwner) return;

    const generation = primaryTreeGenerationRef.current + 1;
    primaryTreeGenerationRef.current = generation;
    if (primaryWorkspace.status !== "ready" || !primaryWorkspace.root) {
      const leavingReady = primaryWorkspaceReadyRef.current;
      primaryWorkspaceReadyRef.current = false;
      if (!leavingReady && !primaryWorkspaceDeferredCleanupPendingRef.current) return;

      if (primaryWorkspace.status === "loading") {
        primaryWorkspaceDeferredCleanupPendingRef.current = true;
      } else {
        primaryWorkspaceDeferredCleanupPendingRef.current = false;
      }
      clearProjectRoot();
      clearOpenDocument({
        openBlank: primaryWorkspace.status === "deferred",
        persistWorkspace: false
      });
      return;
    }

    primaryWorkspaceReadyRef.current = true;
    primaryWorkspaceDeferredCleanupPendingRef.current = false;
    const root = primaryWorkspace.root;
    const openPrimaryRoot = async () => {
      let restoreDocumentPath: string | null = null;
      if (compactMode.trueMobile) {
        const workspace = await getStoredWorkspaceState();
        restoreDocumentPath = workspace.filePath
          ? managedDocumentAbsolutePath(root, workspace.filePath)
          : null;
      }
      if (generation !== primaryTreeGenerationRef.current) return;

      const openedRoot = await openPrimaryFolderPathRef.current(
        root,
        pathNameFromPath(root),
        false,
        true,
        { managed: true, restoreDocumentPath }
      );
      if (!openedRoot || generation !== primaryTreeGenerationRef.current) return;
      if (!compactMode.trueMobile) return;

      if (openedRoot.restoreDocument) {
        const opened = await openManagedTreeMarkdownFile(openedRoot.restoreDocument);
        if (opened === false) {
          clearManagedDocument();
          await persistManagedDocumentPath(null);
        }
        return;
      }

      clearManagedDocument();
      if (restoreDocumentPath) await persistManagedDocumentPath(null);
    };

    openPrimaryRoot().catch(() => {});
    return () => {
      if (primaryTreeGenerationRef.current === generation) {
        primaryTreeGenerationRef.current += 1;
      }
    };
  }, [
    clearManagedDocument,
    clearOpenDocument,
    clearProjectRoot,
    compactMode.trueMobile,
    openPrimaryFolderPathEffectDependency,
    openManagedTreeMarkdownFile,
    persistManagedDocumentPath,
    primaryWindowOwner,
    primaryWorkspace.root,
    primaryWorkspace.status
  ]);
  useEffect(() => {
    if (!compactMode.trueMobile || primaryWorkspace.status !== "ready" || !primaryWorkspace.root) return;

    const relativePath = document.path
      ? managedDocumentRelativePath(primaryWorkspace.root, document.path)
      : null;
    persistManagedDocumentPath(relativePath).catch(() => {});
  }, [
    compactMode.trueMobile,
    document.path,
    persistManagedDocumentPath,
    primaryWorkspace.root,
    primaryWorkspace.status
  ]);
  const appSyncRunRef = useRef(appSync.run);
  appSyncRunRef.current = appSync.run;
  useEffect(() => {
    if (!compactMode.trueMobile || !primaryWorkspace.root) return;

    const refreshOnForeground = () => {
      if (window.document.visibilityState !== "visible") return;

      refreshMarkdownFileTree(activeDocumentPathRef.current).catch(() => {});
      appSyncRunRef.current("app-launch").catch(() => {});
    };
    window.document.addEventListener("visibilitychange", refreshOnForeground);
    return () => window.document.removeEventListener("visibilitychange", refreshOnForeground);
  }, [compactMode.trueMobile, primaryWorkspace.root, refreshMarkdownFileTree]);
  const resolveAssetContextForDocument = useCallback((targetDocumentPath: string | null) => (
    resolveEditorAssetContext({
      documentPath: targetDocumentPath,
      primaryWorkspaceRoot: primaryWorkspace.root
    })
  ), [primaryWorkspace.root]);
  documentRevisionRef.current = document.revision;
  const activeDocumentOutlineIndex =
    !sourceSurfaceActive &&
    activeOutlineIndex !== null &&
    activeOutlineIndex >= 0 &&
    activeOutlineIndex < outlineItems.length
      ? activeOutlineIndex
      : null;
  visualEditorReadyDetailRef.current = {
    chars: document.content.length,
    path: document.path,
    sizeBytes: document.sizeBytes ?? null
  };
  const viewModeChrome = useMemo(
    () => resolveViewModeChrome(
      editorPreferences.preferences.viewMode,
      editorPreferences.preferences.viewModeCustomizations
    ),
    [
      editorPreferences.preferences.viewMode,
      editorPreferences.preferences.viewModeCustomizations
    ]
  );
  const documentLinksOpen = editorPreferences.preferences.documentLinksOpen;
  // View mode is a visibility filter: feature availability and existing user
  // preferences still win, and view mode can only hide what they allow.
  const documentLinksVisible = viewModeChrome.documentLinks && editorPreferences.preferences.documentLinksVisible;
  const fileTreeContentVisible =
    viewModeChrome.fileList ||
    viewModeChrome.outline ||
    documentLinksVisible;
  const visibleFileTreeOpen = viewModeChrome.fileTree && fileTreeContentVisible && fileTreeOpen;
  const compactViewport = !nativeRuntimeAvailable && viewportWidth <= 900;
  const visibleWorkspaceLayoutStyle = {
    ...workspaceLayoutStyle,
    gridTemplateColumns: visibleFileTreeOpen && !compactViewport
      ? `${fileTreeWidth}px minmax(0,1fr)`
      : "0px minmax(0,1fr)"
  } satisfies CSSProperties;
  const sidebarLayoutMode = editorPreferences.preferences.sidebarLayoutMode;
  const documentLinksIndexEnabled = viewModeChrome.fileTree && documentLinksVisible === true && (
    sidebarLayoutMode === "tabs" || documentLinksOpen === true
  );
  const workspaceLinkIndex = useWorkspaceLinkIndex({
    deferMs: workspaceLinkIndexDeferMs,
    documentContent: document.content,
    documentPath: document.path,
    enabled: documentLinksIndexEnabled,
    fileTreeFiles
  });
  const handleDocumentLinksOpenChange = useCallback((openDocumentLinks: boolean) => {
    if (openDocumentLinks === editorPreferences.preferences.documentLinksOpen) return;

    const nextPreferences = {
      ...editorPreferences.preferences,
      documentLinksOpen: openDocumentLinks
    };

    saveStoredEditorPreferences(nextPreferences)
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences.preferences]);
  useEffect(() => {
    setSelectedWordCount(null);
  }, [activeImageFile?.path, activeTabId, editorMode]);
  const handleMainVisualEditorReady = useCallback((
    tabId: string,
    readyEditor: EditorView | null,
    disposedEditor?: EditorView,
    options?: Parameters<typeof handleCodeMirrorEditorReady>[1]
  ) => {
    if (readyEditor) {
      mainVisualEditorsRef.current.set(tabId, readyEditor);
    } else if (mainVisualEditorsRef.current.get(tabId) === disposedEditor) {
      mainVisualEditorsRef.current.delete(tabId);
    } else {
      return;
    }

    const tab = readyEditor ? documentTabs.find((candidate) => candidate.id === tabId) : null;
    if (readyEditor && tab && !tab.dirty) {
      rememberMarkdownTabVisualBaseline(tabId, getMarkdownFromEditor(readyEditor, tab.content));
    }

    if (tabId === activeTabId) {
      handleVisualEditorReady(readyEditor, options);
    }
  }, [
    activeTabId,
    documentTabs,
    getMarkdownFromEditor,
    handleVisualEditorReady,
    rememberMarkdownTabVisualBaseline
  ]);
  useEffect(() => {
    if (!activeTabId) {
      handleVisualEditorReady(null);
      return;
    }

    const activeEditor = mainVisualEditorsRef.current.get(activeTabId);
    if (!activeEditor) return;

    handleVisualEditorReady(activeEditor, { autoFocus: false });
  }, [activeTabId, document.revision, handleVisualEditorReady]);
  useEffect(() => {
    setActiveOutlineIndex(null);
  }, [activeTabId, document.path]);
  const workspaceSearch = useWorkspaceSearch({
    activeImageFile,
    documentContent: document.content,
    documentPath: document.path,
    fileTreeFiles,
    fileTreeSourcePath,
    globalIgnoreRules: fileIgnoreSettings.settings.rules
  });
  const {
    caseSensitive: globalSearchCaseSensitive,
    closeSearch: closeGlobalSearch,
    hideSearch: hideGlobalSearch,
    loading: globalSearchLoading,
    open: globalSearchOpen,
    openSearch: openGlobalSearch,
    query: globalSearchQuery,
    recentQueries: globalSearchRecentQueries,
    response: globalSearchResponse,
    selectRecentQuery: selectGlobalSearchRecentQuery,
    setCaseSensitive: setGlobalSearchCaseSensitive,
    setQuery: setGlobalSearchQuery
  } = workspaceSearch;
  const hasOpenDocument = document.open;
  const largeMarkdownVisualBlocked =
    hasOpenDocument && !activeImageFile && shouldBlockLargeMarkdownVisual(document.content, {
      sizeBytes: document.sizeBytes
    });
  largeMarkdownVisualBlockedRef.current = largeMarkdownVisualBlocked;
  const startupSettingsReady =
    appLanguage.ready &&
    appTheme.ready &&
    !editorPreferences.loading &&
    !fileIgnoreSettings.loading;
  const startupWindowReady =
    startupSettingsReady &&
    (
      onboardingVisible ||
      Boolean(activeImageFile) ||
      !hasOpenDocument ||
      sourceSurfaceActive ||
      largeMarkdownVisualBlocked ||
      visualEditorReadyRevisionRef.current === documentRevisionRef.current
    );
  useStartupWindowReveal({
    enabled: appFeatures.nativeWindowChrome,
    ready: startupWindowReady
  });
  const documentHistoryAvailable = hasOpenDocument && document.path !== null && !activeImageFile && !mainEditorReadOnly;
  const documentSearchAvailable = hasOpenDocument && !activeImageFile;
  const documentSearchSurface: EditorSurface =
    sourceSurfaceActive || largeMarkdownVisualBlocked ? "source" : "visual";
  const {
    activeIndex: normalizedDocumentSearchActiveIndex,
    activeMatch: activeDocumentSearchMatch,
    caseSensitive: documentSearchCaseSensitive,
    close: closeDocumentSearch,
    hide: hideDocumentSearch,
    matchCount: documentSearchMatchCount,
    matches: documentSearchMatches,
    navigate: navigateDocumentSearch,
    open: documentSearchOpen,
    openReplace: openDocumentReplace,
    openSearch: openDocumentSearch,
    query: documentSearchQuery,
    replacement: documentSearchReplacement,
    replaceOpen: documentSearchReplaceOpen,
    resetActiveIndex: resetDocumentSearchActiveIndex,
    revealRevision: documentSearchRevealRevision,
    selectMatch: selectDocumentSearchMatch,
    setCaseSensitive: setDocumentSearchCaseSensitive,
    setReplacement: setDocumentSearchReplacement,
    setReplaceOpen: setDocumentSearchReplaceOpen,
    setQuery: setDocumentSearchQuery,
    setVisualMatches: setVisualDocumentSearchMatches,
    visibleSourceMatches: visibleSourceDocumentSearchMatches,
    visualMatches: visualDocumentSearchMatches
  } = useDocumentSearchState({
    available: documentSearchAvailable,
    sourceContent: document.content,
    surface: documentSearchSurface
  });
  const {
    isApplyingSourceToVisualSync,
    markSourceEditForHistory,
    syncSourceEditsToVisualHistory
  } = useSharedEditorHistory({
    documentContent: document.content,
    documentKey: activeTabId,
    documentRevision: document.revision,
    largeMarkdownVisualBlocked,
    replaceEditorMarkdown,
    sourceSurfaceActive,
    syncSourceToVisual: splitMode && activeEditorSurface === "source",
    visualEditorReadySequence
  });
  const commitEditorContentWidth = useCallback((nextWidth: {
    contentWidth: typeof editorPreferences.preferences.contentWidth;
    contentWidthPx: number;
  }) => {
    const nextPreferences = {
      ...editorPreferences.preferences,
      contentWidth: nextWidth.contentWidth,
      contentWidthPx: nextWidth.contentWidthPx
    };

    saveStoredEditorPreferences(nextPreferences)
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences.preferences]);
  const {
    activeWidth: activeEditorContentWidth,
    activeWidthPx: activeEditorContentWidthPx,
    onResizeEnd: handleEditorContentWidthResizeEnd,
    onWidthChange: handleEditorContentWidthChange
  } = useEditorContentWidthState({
    contentWidth: editorPreferences.preferences.contentWidth,
    contentWidthPx: editorPreferences.preferences.contentWidthPx ?? null,
    onCommit: commitEditorContentWidth
  });
  const editorWidthResizerVisible = true;
  const resolvedSplitVisualPanePercent =
    clampNumber(splitVisualPanePercent, splitVisualPanePercentMin, splitVisualPanePercentMax) ?? defaultSplitVisualPanePercent;
  const resolvedSideDocumentMainPanePercent =
    clampNumber(
      sideDocumentMainPanePercent,
      sideDocumentMainPanePercentMin,
      sideDocumentMainPanePercentMax
    ) ?? defaultSideDocumentMainPanePercent;
  const splitSurfaceStyle = useMemo(() => ({
    "--split-visual-pane": `${resolvedSplitVisualPanePercent}fr`,
    "--split-source-pane": `${100 - resolvedSplitVisualPanePercent}fr`
  } as CSSProperties), [resolvedSplitVisualPanePercent]);
  const sideDocumentSurfaceStyle = useMemo(() => ({
    "--side-document-main-pane": `${resolvedSideDocumentMainPanePercent}fr`,
    "--side-document-secondary-pane": `${100 - resolvedSideDocumentMainPanePercent}fr`
  } as CSSProperties), [resolvedSideDocumentMainPanePercent]);
  const resolveExportImageSrc = useMemo(
    () => createMarkdownImageSrcResolver(document.path, { convertFileSrc: localFileUrlFromPath }),
    [document.path]
  );
  const fallbackImagePreviewSrc = useMemo(() => {
    if (!activeImageFile) return "";

    return createMarkdownImageSrcResolver(activeImageFile.path)(activeImageFile.path);
  }, [activeImageFile]);
  const imagePreviewSrc = imagePreviewObjectUrl ?? fallbackImagePreviewSrc;
  useEffect(() => {
    if (!activeImageFile) {
      setImagePreviewObjectUrl(null);
      return;
    }

    let disposed = false;
    let objectUrl: string | null = null;
    setImagePreviewObjectUrl(null);

    readNativeLocalImageFile(activeImageFile.path)
      .then((file) => {
        if (disposed) return;

        objectUrl = URL.createObjectURL(file);
        setImagePreviewObjectUrl(objectUrl);
      })
      .catch(() => {
        if (!disposed) setImagePreviewObjectUrl(null);
      });

    return () => {
      disposed = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [activeImageFile?.path]);
  const titlebarTabs = useMemo<MarkdownTabsBarDocumentItem[]>(() => [
    ...documentTabs,
    ...imageTabs.map((tab) => ({
      dirty: false,
      displayKind: "image" as const,
      id: tab.id,
      name: tab.name,
      path: tab.path
    }))
  ], [documentTabs, imageTabs]);
  const activeTitlebarTabId = activeImageFile ? imageDocumentTabId(activeImageFile.path) : activeTabId;
  const persistOwnedSideDocumentGroup = useCallback((group: StoredWorkspaceSideBySideGroup | null) => {
    if (workspacePersistencePolicy === "isolated") return;
    persistSideDocumentGroup(group);
  }, [workspacePersistencePolicy]);
  const {
    clearSideDocumentGroup,
    documentOperationTarget,
    focusedSideDocumentTabId,
    openSideDocumentGroup,
    persistSideDocumentGroupPathUpdate,
    persistSideDocumentGroupSavedTabPath,
    setDocumentOperationTarget,
    sideDocumentGroup,
    sideDocumentOpen,
    sideDocumentTab,
    titlebarItems
  } = useSideBySideTabs({
    activeImageFileOpen: Boolean(activeImageFile),
    activeTabId,
    documentTabs,
    documentTabsEnabled: editorPreferences.preferences.showDocumentTabs,
    hasOpenDocument,
    loadStoredWorkspaceState: getStoredWorkspaceState,
    persistSideDocumentGroup: persistOwnedSideDocumentGroup,
    restoreReady: !editorPreferences.loading,
    restoreWorkspaceOnStartup: editorPreferences.preferences.restoreWorkspaceOnStartup,
    titlebarTabs
  });
  const sideEditorReadOnly = editorReadOnlyForPath(
    readOnlyMode,
    sideDocumentTab?.path,
    guardedPaths
  );
  const activeEditorReadOnly = editorReadOnlyForTarget(
    documentOperationTarget,
    document.path,
    sideDocumentTab?.path,
    readOnlyMode,
    guardedPaths
  );
  const applyRenamedTreeFile = useCallback((previousPath: string, renamedFile: NativeMarkdownFolderFile) => {
    replaceOpenDocumentFile(previousPath, renamedFile);
    persistSideDocumentGroupPathUpdate({
      nextPath: renamedFile.path,
      previousPath
    });
    setImageTabs((currentTabs) => currentTabs.map((tab) =>
      tab.path === previousPath ? createImageDocumentTab(renamedFile) : tab
    ));
    setActiveImageFile((currentFile) => currentFile?.path === previousPath ? renamedFile : currentFile);
  }, [persistSideDocumentGroupPathUpdate, replaceOpenDocumentFile]);
  const applyMovedTreeFile = useCallback((
    previousFile: NativeMarkdownFolderFile,
    movedFile: NativeMarkdownFolderFile,
    documentUpdate?: MarkdownTreeMoveDocumentUpdate
  ) => {
    const moveFolderFile = (file: NativeMarkdownFolderFile): NativeMarkdownFolderFile => {
      const nextPath = replaceMovedPath(file.path, previousFile.path, movedFile.path);
      if (nextPath === file.path) return file;

      return {
        ...file,
        name: file.path === previousFile.path ? movedFile.name : file.name,
        path: nextPath,
        relativePath: replaceMovedPath(file.relativePath, previousFile.relativePath, movedFile.relativePath)
      };
    };

    replaceMovedOpenDocumentFile(previousFile.path, movedFile, documentUpdate);
    persistSideDocumentGroupPathUpdate({
      nextPath: movedFile.path,
      previousPath: previousFile.path
    });
    setImageTabs((currentTabs) => currentTabs.map((tab) => {
      const movedTab = moveFolderFile(tab);
      return movedTab === tab ? tab : createImageDocumentTab(movedTab);
    }));
    setActiveImageFile((currentFile) => currentFile ? moveFolderFile(currentFile) : currentFile);
  }, [persistSideDocumentGroupPathUpdate, replaceMovedOpenDocumentFile]);
  const moveTreeFileWithLinks = useCallback(async (
    file: NativeMarkdownFolderFile,
    targetParentPath: string | null
  ) => {
    const parent = targetParentPath ?? fileTreeSourcePath;
    const destinationPath = parent ? nativeChildPath(parent, file.name) : null;
    const mutation = { destinationPath, sourcePath: file.path };
    if (guardFileMutation(mutation)) return null;
    const lease = syncPathMutationRegistry.acquire(mutation);
    if (!lease) {
      guardFileMutation(mutation);
      return null;
    }

    try {
      const result = await moveMarkdownTreeFileWithLinks(file, targetParentPath, {
        dirtyContent: file.kind ? null : getDirtyMarkdownFileContent(file.path),
        moveFile: moveMarkdownTreeFileUnchecked,
        readFile: readNativeMarkdownFile,
        saveFile: saveNativeMarkdownFile
      });
      if (result) applyMovedTreeFile(file, result.file, result.document);

      return result?.file ?? null;
    } finally {
      lease.release();
    }
  }, [
    applyMovedTreeFile,
    fileTreeSourcePath,
    getDirtyMarkdownFileContent,
    guardFileMutation,
    moveMarkdownTreeFileUnchecked,
    syncPathMutationRegistry
  ]);
  const saveDocumentTabViewState = useCallback((tabId: string | null | undefined, patch: DocumentTabViewState) => {
    if (!tabId) return;

    const current = documentTabViewStatesRef.current.get(tabId) ?? {};
    documentTabViewStatesRef.current.set(tabId, {
      ...current,
      ...patch
    });
  }, []);
  const captureActiveDocumentViewState = useCallback(() => {
    if (activeImageFile || !activeTabId) return;

    const nextState: DocumentTabViewState = {};
    if (editorMode === "visual" && visualScrollRef.current) {
      nextState.visualScrollTop = visualScrollRef.current.scrollTop;
    } else if (editorMode === "source" && sourceScrollRef.current) {
      nextState.sourceScrollTop = sourceScrollRef.current.scrollTop;
    } else if (editorMode === "split") {
      if (activeEditorSurface === "visual" && visualScrollRef.current) {
        nextState.visualScrollTop = visualScrollRef.current.scrollTop;
      } else if (activeEditorSurface === "source" && sourceScrollRef.current) {
        nextState.sourceScrollTop = sourceScrollRef.current.scrollTop;
      }
    }
    if (Object.keys(nextState).length > 0) saveDocumentTabViewState(activeTabId, nextState);
  }, [activeEditorSurface, activeImageFile, activeTabId, editorMode, saveDocumentTabViewState]);
  const queueEditorModeScroll = useCallback((targetSurface: EditorSurface) => {
    if (!activeTabId) return;

    const sourceElement = targetSurface === "source" ? visualScrollRef.current : sourceScrollRef.current;
    if (!sourceElement) return;

    const maxScrollTop = Math.max(0, sourceElement.scrollHeight - sourceElement.clientHeight);
    pendingEditorModeScrollRef.current = {
      progress: maxScrollTop <= 0 ? 0 : Math.min(1, Math.max(0, sourceElement.scrollTop / maxScrollTop)),
      tabId: activeTabId,
      targetSurface
    };
  }, [activeTabId]);
  const handleOpenEditorLink = useCallback(async (href: string) => {
    const linkedFile = resolveMarkdownDocumentLinkFile(href, document.path, fileTreeFiles);
    const linkedPath = linkedFile?.path ?? resolveMarkdownDocumentLinkPath(href, document.path);
    if (linkedPath) {
      const fallbackFileName = pathNameFromPath(linkedPath);
      captureActiveDocumentViewState();
      setActiveImageFile(null);
      await openTreeMarkdownFile(linkedFile ?? {
        name: fallbackFileName,
        path: linkedPath,
        relativePath: fallbackFileName
      });
      return;
    }

    try {
      await openNativeExternalUrl(href);
    } catch (error) {
      showAppToast({
        description: diagnosticErrorMessage(error),
        message: translate("app.externalLinkOpenFailed"),
        status: "error"
      });
    }
  }, [captureActiveDocumentViewState, document.path, fileTreeFiles, openTreeMarkdownFile, translate]);

  const holdSelection = editor.holdSelection;
  const updateSelectedWordCount = useCallback((selectedText: string | null | undefined) => {
    const count = selectedText?.trim() ? getWordCount(selectedText) : 0;
    setSelectedWordCount(count > 0 ? count : null);
  }, []);
  const handleTextSelectionChange = useCallback((selection: EditorTextSelection | null) => {
    const textSelectionActive = hasEditorTextSelection();
    const activeSelection = selection?.text.trim() && textSelectionActive ? selection : null;
    const textSelectionAnchor = activeSelection
      ? getEditorSelectionAnchor() ?? selectionAnchorFromDomSelection(window.getSelection())
      : null;
    const holdNativeFullSelection = Boolean(
      activeSelection &&
      activeSelection.from === 0 &&
      activeSelection.to === editor.getDocumentEndPosition() &&
      desktopPlatform === "macos" &&
      getAppRuntime().events.isAvailable()
    );

    updateSelectedWordCount(activeSelection?.text);
    setActiveTextSelection(activeSelection);
    clearSelectionToolbarCopySuccess();
    setSelectionToolbarActiveActions([]);
    setSelectionToolbarHeadingLevel(null);

    if (!activeSelection || mainEditorReadOnly) {
      setSelectionToolbarAnchor(null);
      editor.clearSelection();
      return;
    }

    // Windows and browsers already paint native selections; this fallback covers the macOS Tauri full-selection paint gap.
    if (holdNativeFullSelection) {
      holdSelection(activeSelection);
    } else {
      editor.clearSelection();
    }

    syncSelectionToolbarFormattingState();
    setSelectionToolbarAnchorIfChanged(textSelectionAnchor);
  }, [
    clearSelectionToolbarCopySuccess,
    desktopPlatform,
    editor,
    getEditorSelectionAnchor,
    hasEditorTextSelection,
    holdSelection,
    mainEditorReadOnly,
    setSelectionToolbarAnchorIfChanged,
    syncSelectionToolbarFormattingState,
    updateSelectedWordCount
  ]);
  const handleReadOnlyModeToggle = useCallback(() => {
    const nextReadOnlyMode = !readOnlyMode;
    setReadOnlyMode(nextReadOnlyMode);

    if (nextReadOnlyMode) {
      setSelectionToolbarAnchor(null);
      editor.clearSelection();
    }
  }, [editor, readOnlyMode]);
  const persistEditorModePreferenceToggle = useCallback((
    preference: "typewriterModeEnabled" | "vimModeEnabled"
  ) => {
    const previousValue = editorPreferences.preferences[preference];
    const nextPreferences = {
      ...editorPreferences.preferences,
      [preference]: !previousValue
    };

    editorPreferences.updatePreferences(nextPreferences);
    saveStoredEditorPreferences(nextPreferences)
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {
        editorPreferences.updatePreferences((currentPreferences) => {
          if (currentPreferences[preference] !== nextPreferences[preference]) {
            return currentPreferences;
          }
          return {
            ...currentPreferences,
            [preference]: previousValue
          } satisfies EditorPreferences;
        });
        showAppToast({
          message: translate("app.editorPreferencesSaveFailed"),
          status: "error"
        });
      });
  }, [editorPreferences.preferences, editorPreferences.updatePreferences, translate]);
  const handleTypewriterModeToggle = useCallback(() => {
    persistEditorModePreferenceToggle("typewriterModeEnabled");
  }, [persistEditorModePreferenceToggle]);
  const handleVimModeToggle = useCallback(() => {
    persistEditorModePreferenceToggle("vimModeEnabled");
  }, [persistEditorModePreferenceToggle]);
  const selectionToolbarVisible =
    !sourceSurfaceActive &&
    !mainEditorReadOnly &&
    selectionToolbarAnchor !== null &&
    Boolean(activeTextSelection?.text.trim());
  const selectionToolbarLayoutSignature = useMemo(() => [
    visibleFileTreeOpen ? fileTreeWidth : "file-tree-closed",
    fileTreeResizing ? "file-tree-resizing" : "file-tree-idle",
    sideDocumentOpen ? resolvedSideDocumentMainPanePercent : "side-document-closed",
    splitMode ? resolvedSplitVisualPanePercent : "split-closed",
    activeEditorSurface,
    activeEditorContentWidth,
    activeEditorContentWidthPx ?? "auto",
    documentSearchOpen && documentSearchAvailable ? "search-open" : "search-closed",
    documentSearchReplaceOpen ? "replace-open" : "replace-closed",
    editorPreferences.preferences.showDocumentTabs ? "tabs-open" : "tabs-closed"
  ].join(":"), [
    activeEditorContentWidth,
    activeEditorContentWidthPx,
    activeEditorSurface,
    documentSearchAvailable,
    documentSearchOpen,
    documentSearchReplaceOpen,
    editorPreferences.preferences.showDocumentTabs,
    fileTreeResizing,
    fileTreeWidth,
    resolvedSideDocumentMainPanePercent,
    resolvedSplitVisualPanePercent,
    sideDocumentOpen,
    splitMode,
    visibleFileTreeOpen
  ]);
  const refreshSelectionToolbarAnchor = useCallback(() => {
    if (sourceSurfaceActive || mainEditorReadOnly || !activeTextSelection?.text.trim()) return;

    const nextAnchor = getEditorSelectionAnchor() ?? selectionAnchorFromDomSelection(window.getSelection());
    if (!nextAnchor) return;

    syncSelectionToolbarFormattingState();
    setSelectionToolbarAnchorIfChanged(nextAnchor);
  }, [
    activeTextSelection,
    getEditorSelectionAnchor,
    mainEditorReadOnly,
    setSelectionToolbarAnchorIfChanged,
    sourceSurfaceActive,
    syncSelectionToolbarFormattingState
  ]);
  useSelectionToolbarAnchorRefresh({
    active: selectionToolbarVisible,
    layoutSignature: selectionToolbarLayoutSignature,
    refresh: refreshSelectionToolbarAnchor
  });
  const handleSelectionToolbarDismiss = useCallback(() => {
    setSelectionToolbarAnchor(null);
    clearSelectionToolbarCopySuccess();
  }, [clearSelectionToolbarCopySuccess]);
  const clearActiveTextSelection = useCallback(() => {
    setActiveTextSelection(null);
    setSelectionToolbarAnchor(null);
    editor.clearSelection();
  }, [editor]);
  const handleSaveClipboardImage = useCallback(async (
    image: File,
    origin: "clipboard" | "drop" | "import" | "remote" = "clipboard",
    targetDocumentPath: string | null = document.path
  ) => {
    if (mainEditorReadOnly) return null;

    let result: Awaited<ReturnType<typeof saveLocalEditorImage>>;
    try {
      result = await saveLocalEditorImage({
        context: resolveAssetContextForDocument(targetDocumentPath),
        documentPath: targetDocumentPath,
        image,
        origin,
        preferences: editorPreferences.preferences
      });
    } catch (error) {
      const description = clipboardImageSaveFailureDescription(error);
      showAppToast({
        ...(description ? { description } : {}),
        message: translate("app.clipboardImageSaveFailed"),
        status: "error"
      });
      return null;
    }

    if (result.status === "skipped") {
      showAppToast({
        message: translate("app.clipboardImageRequiresSavedDocument"),
        status: "error"
      });
      return null;
    }

    try {
      if (result.refreshTree && targetDocumentPath) {
        await refreshMarkdownFileTree(targetDocumentPath);
      }
    } catch {
      showAppToast({
        message: translate("app.clipboardImageSaveFailed"),
        status: "error"
      });
      return null;
    }

    return result.image;
  }, [
    document.path,
    editorPreferences.preferences,
    mainEditorReadOnly,
    refreshMarkdownFileTree,
    resolveAssetContextForDocument,
    translate
  ]);

  const handleSaveRemoteClipboardImage = useCallback(async (
    image: RemoteClipboardImage,
    targetDocumentPath: string | null = document.path
  ) => {
    if (mainEditorReadOnly) return null;

    const context = resolveAssetContextForDocument(targetDocumentPath);
    const saved = await persistRemoteEditorImage({
      alt: image.alt,
      context,
      download: (src) => downloadNativeWebImage({ src }),
      save: (downloadedImage) => handleSaveClipboardImage(downloadedImage, "remote", targetDocumentPath),
      url: image.src
    }).catch(() => null);
    if (!saved) {
      showAppToast({
        message: translate("app.clipboardImageSaveFailed"),
        status: "error"
      });
      return null;
    }

    return saved;
  }, [document.path, handleSaveClipboardImage, mainEditorReadOnly, resolveAssetContextForDocument, translate]);

  const handleSaveClipboardAttachment = useCallback(async (
    attachment: File,
    targetDocumentPath: string | null | undefined = document.path,
    origin: "clipboard" | "drop" | "import" = "clipboard"
  ) => {
    if (mainEditorReadOnly) return null;

    const context = resolveAssetContextForDocument(targetDocumentPath ?? null);
    const copyToStorage = resolveEditorAssetAction({
      mode: context.mode,
      origin
    }) !== "reference";
    if (copyToStorage && !targetDocumentPath) {
      showAppToast({
        message: translate("app.clipboardAttachmentRequiresSavedDocument"),
        status: "error"
      });
      return null;
    }

    const savedAttachment = await saveNativeClipboardAttachment({
      attachment,
      copyToStorage,
      documentPath: targetDocumentPath ?? null,
      folder: "assets",
      ...(context.mode === "primary-workspace"
        ? { projectRootPath: context.primaryRootPath }
        : {})
    }).catch(() => null);
    if (!savedAttachment) {
      showAppToast({
        message: translate("app.clipboardAttachmentSaveFailed"),
        status: "error"
      });
      return null;
    }

    if (copyToStorage && targetDocumentPath) {
      await refreshMarkdownFileTree(targetDocumentPath).catch(() => {});
    }

    return savedAttachment;
  }, [
    document.path,
    mainEditorReadOnly,
    refreshMarkdownFileTree,
    resolveAssetContextForDocument,
    translate
  ]);

  const handleSaveEditorResources = useCallback(async (
    request: Parameters<SaveEditorResources>[0],
    targetDocumentPath: string | null = document.path
  ) => {
    const savedResources: Array<Awaited<ReturnType<SaveEditorResources>>[number] | undefined> = [];
    const context = resolveAssetContextForDocument(targetDocumentPath);
    if (request.origin === "remote") {
      const remoteResources: Awaited<ReturnType<SaveEditorResources>> = [];
      for (const src of request.urls) {
        const saved = await handleSaveRemoteClipboardImage(
          { alt: "", src, title: "" },
          targetDocumentPath
        );
        if (saved) remoteResources.push({ ...saved, kind: "image" });
      }
      return remoteResources;
    }

    const normalizedImages = request.files.map((file) => (
      request.origin === "import" &&
      file.type === "application/octet-stream" &&
      nativePathFromEditorFile(file)
        ? null
        : normalizeEditorImageFile(file)
    ));
    const imageIndexes = normalizedImages.flatMap((image, index) => image === null ? [] : [index]);
    const imageAction = resolveEditorAssetAction({ mode: context.mode, origin: request.origin });
    if (imageIndexes.length > 0 && imageAction === "reference") {
      for (const index of imageIndexes) {
        const image = normalizedImages[index];
        if (!image) continue;
        const saved = await handleSaveClipboardImage(image, request.origin, targetDocumentPath);
        if (saved) savedResources[index] = { ...saved, kind: "image" };
      }
    } else if (imageIndexes.length > 0) {
      if (!targetDocumentPath) {
        showAppToast({
          message: translate("app.clipboardImageRequiresSavedDocument"),
          status: "error"
        });
        return [];
      }
      const inputs = await Promise.all(imageIndexes.map(async (index) => {
        const image = normalizedImages[index];
        if (!image) throw new Error("The image batch changed while preparing it.");
        return {
          copyToStorage: true,
          documentPath: targetDocumentPath,
          fileName: await createImageUploadFileName(
            image,
            editorPreferences.preferences.imageUpload.fileNamePattern
          ),
          folder: context.mode === "primary-workspace"
            ? "assets"
            : editorPreferences.preferences.clipboardImageFolder,
          image,
          ...(context.mode === "primary-workspace"
            ? { projectRootPath: context.primaryRootPath }
            : {})
        };
      })).catch(() => null);
      if (inputs === null) {
        showAppToast({
          message: translate("app.clipboardImageSaveFailed"),
          status: "error"
        });
        return [];
      }
      const saved = await appFiles.saveClipboardImages(inputs).catch((error) => {
        const description = clipboardImageSaveFailureDescription(error);
        showAppToast({
          ...(description ? { description } : {}),
          message: translate("app.clipboardImageSaveFailed"),
          status: "error"
        });
        return null;
      });
      if (saved === null) return [];
      if (saved.length !== imageIndexes.length) {
        showAppToast({
          message: translate("app.clipboardImageSaveFailed"),
          status: "error"
        });
        return [];
      }
      for (const [savedIndex, image] of saved.entries()) {
        const requestIndex = imageIndexes[savedIndex];
        if (requestIndex !== undefined) savedResources[requestIndex] = { ...image, kind: "image" };
      }
      await refreshMarkdownFileTree(targetDocumentPath).catch(() => {});
    }

    let refreshImportedAttachmentTreeAfterSave = false;
    for (const [index, file] of request.files.entries()) {
      if (normalizedImages[index] !== null) continue;

      const importedPath = request.origin === "import" ? nativePathFromEditorFile(file) : null;
      if (importedPath) {
        const copyToStorage = resolveEditorAssetAction({
          mode: context.mode,
          origin: request.origin
        }) !== "reference";
        const saved = await importNativeLocalFile({
          copyToStorage,
          documentPath: targetDocumentPath,
          file: { name: file.name, path: importedPath },
          folder: "assets",
          ...(context.mode === "primary-workspace"
            ? { projectRootPath: context.primaryRootPath }
            : {})
        }).catch(() => null);
        if (saved) {
          savedResources[index] = { ...saved, kind: "attachment" };
          refreshImportedAttachmentTreeAfterSave = refreshImportedAttachmentTreeAfterSave || copyToStorage;
        } else {
          showAppToast({
            message: translate("app.clipboardAttachmentSaveFailed"),
            status: "error"
          });
        }
        continue;
      }

      const saved = await handleSaveClipboardAttachment(file, targetDocumentPath, request.origin);
      if (saved) savedResources[index] = { ...saved, kind: "attachment" };
    }

    if (refreshImportedAttachmentTreeAfterSave && targetDocumentPath) {
      await refreshImportedAttachmentTree(() => refreshMarkdownFileTree(targetDocumentPath));
    }
    return savedResources.filter((resource): resource is Awaited<ReturnType<SaveEditorResources>>[number] => resource !== undefined);
  }, [
    appFiles,
    document.path,
    editorPreferences.preferences.clipboardImageFolder,
    editorPreferences.preferences.imageUpload.fileNamePattern,
    handleSaveClipboardAttachment,
    handleSaveClipboardImage,
    handleSaveRemoteClipboardImage,
    refreshMarkdownFileTree,
    resolveAssetContextForDocument,
    translate
  ]);

  const handleOpenLocalAttachment = useCallback(async (src: string, documentPath: string | null | undefined = document.path) => {
    if (!appFeatures.openLocalAttachments) {
      showAppToast({
        message: translate("app.mobileAttachmentUnsupported"),
        status: "error"
      });
      return;
    }

    const rootPath = fileTree.sourcePath ?? documentPath ?? null;
    if (!rootPath && !src.toLowerCase().startsWith("file://")) return;

    await openNativeMarkdownAttachment({
      documentPath: documentPath ?? null,
      rootPath,
      src
    }).catch(() => {
      showAppToast({
        message: translate("app.clipboardImageSaveFailed"),
        status: "error"
      });
    });
  }, [appFeatures.openLocalAttachments, document.path, fileTree.sourcePath, translate]);

  useEffect(() => {
    setSplitVisualPanePercent(editorPreferences.preferences.splitVisualPanePercent);
    pendingSplitVisualPanePercentRef.current = editorPreferences.preferences.splitVisualPanePercent;
  }, [editorPreferences.preferences.splitVisualPanePercent]);

  useEffect(() => {
    return () => {
      splitPaneResizeCleanupRef.current?.();
      splitPaneResizeCleanupRef.current = null;
      sideDocumentPaneResizeCleanupRef.current?.();
      sideDocumentPaneResizeCleanupRef.current = null;
      if (splitScrollSyncFrameRef.current !== null) {
        window.cancelAnimationFrame(splitScrollSyncFrameRef.current);
        splitScrollSyncFrameRef.current = null;
      }
    };
  }, []);

  const resizeSplitVisualPane = useCallback((nextPercent: number | null) => {
    if (nextPercent === null) return;

    const roundedPercent = Math.round(nextPercent);
    pendingSplitVisualPanePercentRef.current = roundedPercent;
    setSplitVisualPanePercent(roundedPercent);
  }, []);
  const handleSplitPaneResizeEnd = useCallback(() => {
    const nextPercent = pendingSplitVisualPanePercentRef.current;
    if (nextPercent === editorPreferences.preferences.splitVisualPanePercent) return;

    const nextPreferences = {
      ...editorPreferences.preferences,
      splitVisualPanePercent: nextPercent
    };

    saveStoredEditorPreferences(nextPreferences)
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences.preferences]);
  const handleSplitPaneResizePointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const splitSurface = splitSurfaceRef.current;
    if (!splitSurface) return;

    const surfaceRect = splitSurface.getBoundingClientRect();
    if (surfaceRect.width <= 0) return;

    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture?.(event.pointerId);

    const startX = event.clientX;
    const startPercent = pendingSplitVisualPanePercentRef.current;
    const previousCursor = window.document.body.style.cursor;
    const previousUserSelect = window.document.body.style.userSelect;

    window.document.body.style.cursor = "col-resize";
    window.document.body.style.userSelect = "none";

    const handlePointerMove = (moveEvent: PointerEvent) => {
      resizeSplitVisualPane(clampNumber(
        startPercent + ((moveEvent.clientX - startX) / surfaceRect.width) * 100,
        splitVisualPanePercentMin,
        splitVisualPanePercentMax
      ));
    };

    const cleanup = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerUp);
      window.document.body.style.cursor = previousCursor;
      window.document.body.style.userSelect = previousUserSelect;
      splitPaneResizeCleanupRef.current = null;
      handleSplitPaneResizeEnd();
    };

    const handlePointerUp = () => {
      cleanup();
    };

    splitPaneResizeCleanupRef.current?.();
    splitPaneResizeCleanupRef.current = cleanup;
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerUp);
  }, [handleSplitPaneResizeEnd, resizeSplitVisualPane]);
  const handleSplitPaneResizeKeyDown = useCallback((event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      resizeSplitVisualPane(clampNumber(
        splitVisualPanePercent - splitPaneKeyboardStepPercent,
        splitVisualPanePercentMin,
        splitVisualPanePercentMax
      ));
      handleSplitPaneResizeEnd();
      return;
    }

    if (event.key === "ArrowRight") {
      event.preventDefault();
      resizeSplitVisualPane(clampNumber(
        splitVisualPanePercent + splitPaneKeyboardStepPercent,
        splitVisualPanePercentMin,
        splitVisualPanePercentMax
      ));
      handleSplitPaneResizeEnd();
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      resizeSplitVisualPane(splitVisualPanePercentMin);
      handleSplitPaneResizeEnd();
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      resizeSplitVisualPane(splitVisualPanePercentMax);
      handleSplitPaneResizeEnd();
    }
  }, [handleSplitPaneResizeEnd, resizeSplitVisualPane, splitVisualPanePercent]);
  const resizeSideDocumentMainPane = useCallback((nextPercent: number | null) => {
    if (nextPercent === null) return;

    const roundedPercent = Math.round(nextPercent);
    pendingSideDocumentMainPanePercentRef.current = roundedPercent;
    setSideDocumentMainPanePercent(roundedPercent);
  }, []);
  const handleSideDocumentPaneResizePointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const sideDocumentSurface = sideDocumentSurfaceRef.current;
    if (!sideDocumentSurface) return;

    const surfaceRect = sideDocumentSurface.getBoundingClientRect();
    if (surfaceRect.width <= 0) return;

    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture?.(event.pointerId);

    const startX = event.clientX;
    const startPercent = pendingSideDocumentMainPanePercentRef.current;
    const previousCursor = window.document.body.style.cursor;
    const previousUserSelect = window.document.body.style.userSelect;

    window.document.body.style.cursor = "col-resize";
    window.document.body.style.userSelect = "none";

    const handlePointerMove = (moveEvent: PointerEvent) => {
      resizeSideDocumentMainPane(clampNumber(
        startPercent + ((moveEvent.clientX - startX) / surfaceRect.width) * 100,
        sideDocumentMainPanePercentMin,
        sideDocumentMainPanePercentMax
      ));
    };

    const cleanup = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerUp);
      window.document.body.style.cursor = previousCursor;
      window.document.body.style.userSelect = previousUserSelect;
      sideDocumentPaneResizeCleanupRef.current = null;
    };

    const handlePointerUp = () => {
      cleanup();
    };

    sideDocumentPaneResizeCleanupRef.current?.();
    sideDocumentPaneResizeCleanupRef.current = cleanup;
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerUp);
  }, [resizeSideDocumentMainPane]);
  const handleSideDocumentPaneResizeKeyDown = useCallback((event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      resizeSideDocumentMainPane(clampNumber(
        sideDocumentMainPanePercent - sideDocumentPaneKeyboardStepPercent,
        sideDocumentMainPanePercentMin,
        sideDocumentMainPanePercentMax
      ));
      return;
    }

    if (event.key === "ArrowRight") {
      event.preventDefault();
      resizeSideDocumentMainPane(clampNumber(
        sideDocumentMainPanePercent + sideDocumentPaneKeyboardStepPercent,
        sideDocumentMainPanePercentMin,
        sideDocumentMainPanePercentMax
      ));
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      resizeSideDocumentMainPane(sideDocumentMainPanePercentMin);
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      resizeSideDocumentMainPane(sideDocumentMainPanePercentMax);
    }
  }, [resizeSideDocumentMainPane, sideDocumentMainPanePercent]);
  const handleTitlebarActionsChange = useCallback((titlebarActions: TitlebarActionPreference[]) => {
    const nextPreferences = {
      ...editorPreferences.preferences,
      titlebarActions
    };

    saveStoredEditorPreferences(nextPreferences)
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences.preferences]);
  const handleViewModeSelect = useCallback((viewMode: ViewMode) => {
    if (viewMode === editorPreferences.preferences.viewMode) return;

    const nextPreferences = {
      ...editorPreferences.preferences,
      viewMode
    };

    editorPreferences.updatePreferences(nextPreferences);
    saveStoredEditorPreferences(nextPreferences)
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences.preferences, editorPreferences.updatePreferences]);
  const handleViewModeCycle = useCallback(() => {
    handleViewModeSelect(nextViewMode(editorPreferences.preferences.viewMode));
  }, [editorPreferences.preferences.viewMode, handleViewModeSelect]);
  const handleCreateMarkdownTreeFile = useCallback(async (
    fileName: string,
    parentPath: string | null = null,
    contents?: string
  ) => {
    try {
      if (!fileTree.sourcePath) {
        captureActiveDocumentViewState();
        setActiveImageFile(null);
        await createBlankDocument({
          content: contents ?? "",
          name: unsavedMarkdownFileNameFromTreeInput(fileName)
        });
        return;
      }

      const file = await createMarkdownTreeFile(fileName, parentPath, contents);
      if (file) {
        captureActiveDocumentViewState();
        setActiveImageFile(null);
        await openTreeMarkdownFile(file);
      }
    } catch (error) {
      showAppToast({
        message: nativeFileOperationFailureMessage(translate("app.markdownFileCreateFailed"), error),
        status: "error"
      });
    }
  }, [captureActiveDocumentViewState, createBlankDocument, createMarkdownTreeFile, fileTree.sourcePath, openTreeMarkdownFile, translate]);
  const handleQuickCreateMarkdownTreeFile = useCallback(() => {
    captureActiveDocumentViewState();
    setActiveImageFile(null);
    createBlankDocument().catch(() => {});
  }, [captureActiveDocumentViewState, createBlankDocument]);
  const openImageTab = useCallback((file: NativeMarkdownFolderFile) => {
    const tab = createImageDocumentTab(file);
    setImageTabs((currentTabs) =>
      currentTabs.some((currentTab) => currentTab.id === tab.id)
        ? currentTabs.map((currentTab) => currentTab.id === tab.id ? tab : currentTab)
        : [...currentTabs, tab]
    );
    setActiveImageFile(file);
  }, []);
  const handleCreateMarkdownTreeFolder = useCallback(async (folderName: string, parentPath: string | null = null) => {
    try {
      await createMarkdownTreeFolder(folderName, parentPath);
    } catch (error) {
      showAppToast({
        message: nativeFileOperationFailureMessage(translate("app.markdownFolderCreateFailed"), error),
        status: "error"
      });
    }
  }, [createMarkdownTreeFolder, translate]);
  const handleRenameMarkdownTreeFile = useCallback(async (file: NativeMarkdownFolderFile, fileName: string) => {
    try {
      const renamedFile = await renameMarkdownTreeFile(file, fileName);
      if (renamedFile) applyRenamedTreeFile(file.path, renamedFile);
    } catch (error) {
      showAppToast({
        message: nativeFileOperationFailureMessage(translate("app.markdownFileRenameFailed"), error),
        status: "error"
      });
    }
  }, [applyRenamedTreeFile, renameMarkdownTreeFile, translate]);
  const handleRenameCompactFile = useCallback(async (
    file: NativeMarkdownFolderFile,
    fileName: string
  ) => {
    const renamedFile = await renameMarkdownTreeFile(file, fileName);
    if (!renamedFile) return;
    applyRenamedTreeFile(file.path, renamedFile);
  }, [applyRenamedTreeFile, renameMarkdownTreeFile]);
  const handleMoveMarkdownTreeFile = useCallback(async (
    file: NativeMarkdownFolderFile,
    targetParentPath: string | null
  ) => {
    try {
      const movedFile = await moveTreeFileWithLinks(file, targetParentPath);
      if (!movedFile) return false;
      return true;
    } catch {
      // Keep the existing tree state if the native move fails.
      return false;
    }
  }, [moveTreeFileWithLinks]);
  const handleDeleteMarkdownTreeFile = useCallback(async (
    file: NativeMarkdownFolderFile,
    context?: { files: readonly NativeMarkdownFolderFile[] }
  ) => {
    const deleteTargets = context?.files?.length ? context.files : [file];
    const uniqueDeleteTargets = deleteTargets.filter((target, index, targets) =>
      targets.findIndex((candidate) => sameNativePath(candidate.path, target.path)) === index
    );
    if (uniqueDeleteTargets.some((target) => guardFileMutation({ sourcePath: target.path }))) return;
    const deletingMultipleFiles = uniqueDeleteTargets.length > 1;
    const fileIsFolder = file.kind === "folder";
    const deleteCount = String(uniqueDeleteTargets.length);
    const deleteCountLabel = translate("app.workspaceSearch.fileCountPlural").replace("{count}", deleteCount);
    const confirmed = await confirmNativeMarkdownFileDelete(deletingMultipleFiles ? deleteCountLabel : file.name, {
      cancelLabel: translate(fileIsFolder ? "app.cancelDeleteMarkdownFolder" : "app.cancelDeleteMarkdownFile"),
      message: deletingMultipleFiles
        ? translate("app.confirmDeleteSelectedMarkdownFiles").replace("{count}", deleteCount)
        : translate(fileIsFolder ? "app.confirmDeleteMarkdownFolder" : "app.confirmDeleteMarkdownFile"),
      okLabel: translate(fileIsFolder ? "app.confirmDeleteMarkdownFolderAction" : "app.confirmDeleteMarkdownFileAction")
    });
    if (!confirmed) return;

    for (const targetFile of uniqueDeleteTargets) {
      try {
        const deleted = await deleteMarkdownTreeFile(targetFile);
        if (deleted) detachDeletedDocumentFile(targetFile.path);
      } catch {
        // Leave the file visible when native deletion fails.
      }
    }
  }, [deleteMarkdownTreeFile, detachDeletedDocumentFile, guardFileMutation, translate]);
  const handleSaveMarkdownFileAsTemplate = useCallback(async (file: NativeMarkdownFolderFile) => {
    if (file.kind === "asset" || file.kind === "attachment" || file.kind === "folder") return;

    try {
      const markdownFile = await readNativeMarkdownFile(file.path);
      if (!markdownFile.content.trim()) {
        showAppToast({
          message: translate("app.markdownTemplateSaveFailed"),
          status: "error"
        });
        return;
      }

      const template = createCustomMarkdownTemplateFromFile(markdownFile, [
        ...editorPreferences.preferences.markdownTemplates,
        ...markdownTemplates
      ]);
      const templateEntry = markdownTemplateEntryFromTemplate(template);
      const storedMarkdownTemplates = [
        ...editorPreferences.preferences.markdownTemplates,
        templateEntry
      ];
      const nextPreferences = {
        ...editorPreferences.preferences,
        markdownTemplates: storedMarkdownTemplates
      };

      await writeNativeMarkdownTemplateFile(templateEntry.fileName, template.content);
      await saveStoredEditorPreferences(nextPreferences);
      notifyAppEditorPreferencesChanged(nextPreferences).catch(() => {});
      setMarkdownTemplates((currentTemplates) => [
        ...currentTemplates,
        {
          ...template,
          fileName: templateEntry.fileName
        }
      ]);
      showAppToast({
        message: translate("app.markdownTemplateSaved"),
        status: "success"
      });
    } catch {
      showAppToast({
        message: translate("app.markdownTemplateSaveFailed"),
        status: "error"
      });
    }
  }, [editorPreferences.preferences, markdownTemplates, translate]);
  const handleOpenTreeFile = useCallback(async (
    file: NativeMarkdownFolderFile,
    options: { managed?: boolean } = {}
  ) => {
    captureActiveDocumentViewState();

    if (file.kind === "asset") {
      openImageTab(file);
      return true;
    }

    if (file.kind === "attachment") {
      await handleOpenLocalAttachment(file.relativePath, null);
      return appFeatures.openLocalAttachments;
    }

    setActiveImageFile(null);
    if (options.managed) {
      await openManagedTreeMarkdownFile(file);
      return true;
    }

    await openTreeMarkdownFile(file);
    return true;
  }, [appFeatures.openLocalAttachments, captureActiveDocumentViewState, handleOpenLocalAttachment, openImageTab, openManagedTreeMarkdownFile, openTreeMarkdownFile]);
  const handleCreateCompactDocument = useCallback(async (fileName: string) => {
    if (!fileTree.sourcePath) {
      return createBlankDocument({
        name: unsavedMarkdownFileNameFromTreeInput(fileName)
      });
    }

    const file = await createMarkdownTreeFile(fileName, null);
    if (!file) return false;

    await handleOpenTreeFile(file, { managed: compactMode.trueMobile });
    return true;
  }, [compactMode.trueMobile, createBlankDocument, createMarkdownTreeFile, fileTree.sourcePath, handleOpenTreeFile]);
  const handleQuickOpenOpen = useCallback(() => {
    hideGlobalSearch();
    hideDocumentSearch();
    setQuickOpenOpen(true);
  }, [hideDocumentSearch, hideGlobalSearch]);
  const handleQuickOpenClose = useCallback(() => {
    setQuickOpenOpen(false);
  }, []);
  const handleGlobalSearchOpen = useCallback(() => {
    setQuickOpenOpen(false);
    openGlobalSearch();
  }, [openGlobalSearch]);
  const handleGlobalSearchClose = closeGlobalSearch;
  const handleGlobalSearchQueryChange = useCallback((query: string) => {
    setGlobalSearchQuery(query);
  }, [setGlobalSearchQuery]);
  const handleGlobalSearchCaseSensitiveChange = useCallback((caseSensitive: boolean) => {
    setGlobalSearchCaseSensitive(caseSensitive);
  }, [setGlobalSearchCaseSensitive]);
  const handleGlobalSearchRecentQuerySelect = useCallback((query: string) => {
    selectGlobalSearchRecentQuery(query);
  }, [selectGlobalSearchRecentQuery]);
  const handleGlobalSearchResultOpen = useCallback(async (result: WorkspaceSearchResult) => {
    hideGlobalSearch();
    await handleOpenTreeFile(result.file);
    if (!documentSearchOpen) return;

    setDocumentSearchQuery(globalSearchQuery.trim());
    setDocumentSearchCaseSensitive(globalSearchCaseSensitive);
    setDocumentSearchReplaceOpen(false);
    selectDocumentSearchMatch(result.matchIndex);
  }, [
    documentSearchOpen,
    globalSearchCaseSensitive,
    globalSearchQuery,
    handleOpenTreeFile,
    hideGlobalSearch,
    selectDocumentSearchMatch,
    setDocumentSearchCaseSensitive,
    setDocumentSearchQuery,
    setDocumentSearchReplaceOpen
  ]);
  const handleOpenTreeFileToSide = useCallback(async (file: NativeMarkdownFolderFile) => {
    captureActiveDocumentViewState();

    if (file.kind === "asset") {
      openImageTab(file);
      return;
    }

    if (file.kind === "attachment") {
      await handleOpenLocalAttachment(file.relativePath, null);
      return;
    }

    if (!editorPreferences.preferences.showDocumentTabs || activeImageFile || !hasOpenDocument) {
      setActiveImageFile(null);
      await openTreeMarkdownFile(file);
      return;
    }

    const tabId = await openTreeMarkdownFileInBackground(file);
    if (!activeTabId || !tabId || tabId === activeTabId) return;

    clearActiveTextSelection();
    if (splitMode) {
      setEditorMode("visual");
      setActiveEditorSurface("visual");
    }
    const primaryFilePath = documentTabs.find((tab) => tab.id === activeTabId)?.path ?? document.path;
    openSideDocumentGroup({
      primaryFilePath,
      primaryTabId: activeTabId,
      sideFilePath: file.path,
      sideTabId: tabId
    });
  }, [
    activeImageFile,
    activeTabId,
    captureActiveDocumentViewState,
    clearActiveTextSelection,
    handleOpenLocalAttachment,
    hasOpenDocument,
    document.path,
    documentTabs,
    editorPreferences.preferences.showDocumentTabs,
    openSideDocumentGroup,
    openImageTab,
    openTreeMarkdownFile,
    openTreeMarkdownFileInBackground,
    splitMode
  ]);
  const handleQuickOpenFileOpen = useCallback(async (
    file: NativeMarkdownFolderFile,
    options: { toSide: boolean }
  ) => {
    setQuickOpenOpen(false);

    if (options.toSide) {
      await handleOpenTreeFileToSide(file);
      return;
    }

    await handleOpenTreeFile(file);
  }, [handleOpenTreeFile, handleOpenTreeFileToSide]);
  const openExternalFileInNewWindow = useCallback(async () => {
    try {
      const file = await openNativeMarkdownFile({ title: translate("app.openMarkdownFile") });
      if (!file) return null;
      await openNativeMarkdownFileInNewWindow(file.path);
      return file.path;
    } catch {
      return null;
    }
  }, [translate]);
  const handleOpenMarkdownFile = useCallback(async () => {
    if (primaryWindowOwner && !compactMode.trueMobile) {
      await openExternalFileInNewWindow();
      return;
    }

    captureActiveDocumentViewState();
    setActiveImageFile(null);
    await openMarkdownFile({
      pickerTitle: translate("app.openMarkdownFile")
    });
  }, [
    captureActiveDocumentViewState,
    compactMode.trueMobile,
    openExternalFileInNewWindow,
    openMarkdownFile,
    primaryWindowOwner,
    translate
  ]);
  const handleOpenRecentMarkdownFile = useCallback(async (file: RecentMarkdownFile) => {
    if (
      primaryWindowOwner &&
      !compactMode.trueMobile &&
      (!primaryRoot || managedDocumentRelativePath(primaryRoot, file.path) === null)
    ) {
      try {
        await openNativeMarkdownFileInNewWindow(file.path);
      } catch {
        // Keep the primary window unchanged when external window creation fails.
      }
      return;
    }

    captureActiveDocumentViewState();
    setActiveImageFile(null);
    await openRecentMarkdownFile(file);
  }, [
    captureActiveDocumentViewState,
    compactMode.trueMobile,
    openRecentMarkdownFile,
    primaryRoot,
    primaryWindowOwner
  ]);
  const handleCloseCurrentFile = useCallback(async () => {
    captureActiveDocumentViewState();

    const focusedSideCloseTabId =
      documentOperationTarget === "side" &&
      !activeImageFile &&
      sideDocumentGroup?.primaryTabId === activeTabId
        ? sideDocumentGroup.sideTabId
        : null;

    if (focusedSideCloseTabId) {
      const closed = await closeMarkdownTab(focusedSideCloseTabId);
      if (!closed) return;

      clearSideDocumentGroup();
      clearActiveTextSelection();
      return;
    }

    if (activeImageFile) {
      const closingTabId = imageDocumentTabId(activeImageFile.path);
      setImageTabs((currentTabs) => currentTabs.filter((tab) => tab.id !== closingTabId));
      setActiveImageFile(null);
      return;
    }

    if (activeTabId) {
      const closed = await closeMarkdownTab(activeTabId);
      if (!closed) return;

      if (sideDocumentGroup?.primaryTabId === activeTabId || sideDocumentGroup?.sideTabId === activeTabId) {
        clearSideDocumentGroup();
      }
      clearActiveTextSelection();
      return;
    }

    const canDiscard = await confirmCanDiscardCurrentDocument();
    if (!canDiscard) return;

    clearActiveTextSelection();
    clearOpenDocument();
  }, [
    activeImageFile,
    activeTabId,
    captureActiveDocumentViewState,
    clearSideDocumentGroup,
    clearOpenDocument,
    clearActiveTextSelection,
    closeMarkdownTab,
    confirmCanDiscardCurrentDocument,
    documentOperationTarget,
    sideDocumentGroup
  ]);
  const handleFileTreeToggle = useCallback(() => toggleFileTree(document.path), [document.path, toggleFileTree]);
  const [fileTreeRevealPathRequest, setFileTreeRevealPathRequest] = useState<{ id: number; path: string } | null>(null);
  const fileTreeRevealPathRequestIdRef = useRef(0);
  const handleRevealPathInFileTree = useCallback((path: string | null | undefined) => {
    const targetPath = path?.trim();
    if (!targetPath) return;

    if (!fileTreeOpen) toggleFileTree(targetPath);

    fileTreeRevealPathRequestIdRef.current += 1;
    setFileTreeRevealPathRequest({
      id: fileTreeRevealPathRequestIdRef.current,
      path: targetPath
    });
  }, [fileTreeOpen, toggleFileTree]);
  const handleDocumentHistoryOpen = useCallback(() => {
    if (!documentHistoryAvailable) return;

    setDocumentHistoryOpen((current) => !current);
  }, [documentHistoryAvailable]);
  const handleDocumentHistoryRestore = useCallback(async (contents: string, historyId: string) => {
    debug(() => ["[markra-history] app restore requested", {
      contentsChars: contents.length,
      currentDirty: document.dirty,
      currentPath: document.path,
      currentRevision: document.revision,
      historyId
    }]);

    const mutation = { sourcePath: document.path };
    if (
      syncExistingDocumentWriteBlockedByGuardedPath(document.path, guardedPaths) ||
      syncPathMutationRegistry.isBlocked(mutation)
    ) {
      guardFileMutation(mutation);
      return false;
    }
    const lease = document.path ? syncPathMutationRegistry.acquire(mutation) : null;
    if (document.path && !lease) {
      guardFileMutation(mutation);
      return false;
    }

    try {
      if (document.dirty) {
        const savedCurrent = await saveCurrentDocument();
        if (!savedCurrent) {
          debug(() => ["[markra-history] app restore canceled", {
            historyId,
            reason: "current document was not saved"
          }]);
          return false;
        }
      }

      const restored = restoreDocumentContent(contents);
      debug(() => ["[markra-history] app restore state result", {
        restored
      }]);
      if (!restored) return false;

      const editorReplaced = replaceEditorMarkdown(contents);
      debug(() => ["[markra-history] editor replace requested", {
        editorReplaced
      }]);
      try {
        // A normal save snapshots the version that was current immediately before this restore.
        const savedFile = await saveCurrentDocumentContent(contents);
        debug(() => ["[markra-history] save restored document success", {
          savedPath: savedFile?.path ?? null
        }]);
        return savedFile !== null;
      } catch (error: unknown) {
        debug(() => ["[markra-history] save restored document failed", {
          error: error instanceof Error ? error.message : String(error)
        }]);
        return false;
      }
    } finally {
      lease?.release();
    }
  }, [
    document.dirty,
    document.path,
    document.revision,
    guardedPaths,
    guardFileMutation,
    replaceEditorMarkdown,
    restoreDocumentContent,
    saveCurrentDocument,
    saveCurrentDocumentContent,
    syncPathMutationRegistry
  ]);
  useEffect(() => {
    if (documentHistoryAvailable) return;

    setDocumentHistoryOpen(false);
  }, [documentHistoryAvailable]);
  const refreshOpenDocumentHistory = useCallback((savedPath: string | null) => {
    if (!documentHistoryOpen || savedPath === null || savedPath !== document.path) return;

    setDocumentHistoryRefreshKey((current) => current + 1);
  }, [document.path, documentHistoryOpen]);
  const handleOpenSettings = useCallback(() => {
    openSettingsModal(
      undefined,
      primaryRoot,
      fileTreeSourcePath ?? document.path ?? primaryRoot
    ).catch(() => {});
  }, [document.path, fileTreeSourcePath, openSettingsModal, primaryRoot]);
  const handleOpenBlankEditorWindow = useCallback(() => {
    openBlankEditorWindow().catch(() => {});
  }, []);
  const handleShowAbout = useCallback(() => {
    showNativeAppAbout().catch(() => {});
  }, []);
  const handleExitApp = useCallback(() => {
    closeNativeWindow().catch(() => {});
  }, []);
  const rawFileTreeRootName = rootNameForDocument(document.path);
  const fileTreeRootName =
    rawFileTreeRootName === "No folder"
      ? translate("app.noFolder")
      : rawFileTreeRootName === "Files"
        ? translate("app.files")
        : rawFileTreeRootName;
  const saveDocument = useCallback(async (saveAs = false) => {
    const targetPath = focusedSideDocumentTabId ? sideDocumentTab?.path : document.path;
    const mutation = { sourcePath: targetPath };
    if (
      !saveAs &&
      (
        syncSaveBlockedByGuardedPath(false, targetPath, guardedPaths) ||
        syncPathMutationRegistry.isBlocked(mutation)
      )
    ) {
      guardFileMutation(mutation);
      return null;
    }
    const lease = !saveAs && targetPath ? syncPathMutationRegistry.acquire(mutation) : null;
    if (!saveAs && targetPath && !lease) {
      guardFileMutation(mutation);
      return null;
    }

    try {
      if (focusedSideDocumentTabId) {
        const savedFile = await saveMarkdownTab(focusedSideDocumentTabId, saveAs);
        if (savedFile) persistSideDocumentGroupSavedTabPath(focusedSideDocumentTabId, savedFile.path);
        if (savedFile) await appSync.notifyDocumentSaved(savedFile.path);
        return savedFile;
      }

      const savedFile = await saveCurrentDocument(saveAs);
      if (savedFile && activeTabId) persistSideDocumentGroupSavedTabPath(activeTabId, savedFile.path);
      if (savedFile) refreshOpenDocumentHistory(savedFile.path);
      if (savedFile) await appSync.notifyDocumentSaved(savedFile.path);
      return savedFile;
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes("sync-path-guarded")) {
        showSyncPathGuardedNotice();
        return null;
      }
      throw error;
    } finally {
      lease?.release();
    }
  }, [
    activeTabId,
    document.path,
    focusedSideDocumentTabId,
    guardedPaths,
    guardFileMutation,
    persistSideDocumentGroupSavedTabPath,
    refreshOpenDocumentHistory,
    appSync.notifyDocumentSaved,
    saveCurrentDocument,
    saveMarkdownTab,
    sideDocumentTab?.path,
    showSyncPathGuardedNotice,
    syncPathMutationRegistry
  ]);
  const handleSaveDocument = useCallback(() => saveDocument(false), [saveDocument]);
  const saveDocumentAs = useCallback(() => saveDocument(true), [saveDocument]);
  const resolveSideDocumentImageSrc = useMemo(
    () => editorImageSrcResolverForPath(sideDocumentTab?.path ?? null),
    [editorImageSrcResolverForPath, sideDocumentTab?.path]
  );
  const sideDocumentWordCount = useMemo(
    () => sideDocumentTab ? getWordCount(sideDocumentTab.content) : 0,
    [sideDocumentTab?.content]
  );
  const documentTabsVisible =
    viewModeChrome.documentTabs &&
    editorPreferences.preferences.showDocumentTabs &&
    (hasOpenDocument || Boolean(activeImageFile)) &&
    titlebarTabs.some((tab) => titlebarTabs.length > 1 || tab.path !== null || tab.dirty);
  const currentFileTreePath = activeImageFile?.path ?? (
    focusedSideDocumentTabId && sideDocumentTab?.path
      ? sideDocumentTab.path
      : hasOpenDocument ? document.path : null
  );
  const titleDocumentName = activeImageFile ? activeImageFile.name : hasOpenDocument ? document.name : fileTreeRootName;
  const titleDocumentKind = activeImageFile ? "image" : hasOpenDocument ? "file" : "folder";
  const sourceModeAvailable = hasOpenDocument && !activeImageFile;
  useEffect(() => {
    if (activeEditorSurface !== "source") return;

    sourceFocusSourceEditSequenceRef.current = sourceEditSequenceRef.current;
  }, [activeEditorSurface, activeTabId, document.path, document.revision]);
  const handleMainDocumentPaneFocus = useCallback(() => {
    setDocumentOperationTarget("main");
  }, []);
  const handleVisualPaneFocus = useCallback(() => {
    setDocumentOperationTarget("main");
    setActiveEditorSurface("visual");
  }, []);
  const handleSourcePaneFocus = useCallback(() => {
    setDocumentOperationTarget("main");
    sourceFocusSourceEditSequenceRef.current = sourceEditSequenceRef.current;
    setActiveEditorSurface("source");
    clearActiveTextSelection();
  }, [clearActiveTextSelection]);
  const handleSideDocumentPaneFocus = useCallback(() => {
    setDocumentOperationTarget("side");
  }, []);
  const handleVisualMarkdownTabChange = useCallback((
    tabId: string,
    content: string,
    options?: { documentRevision?: number }
  ) => {
    if (isApplyingSourceToVisualSync()) return;
    if (syncingExternalDocumentHistoryRef.current) return;
    if (sourceMode) return;
    if (mainEditorReadOnly) return;
    if (
      splitMode &&
      activeEditorSurface === "source" &&
      sourceEditSequenceRef.current !== sourceFocusSourceEditSequenceRef.current
    ) {
      return;
    }

    if (splitMode && activeEditorSurface !== "source") setActiveEditorSurface("visual");
    // IME completion can publish after another tab becomes active, so preserve the
    // originating tab identity instead of writing through the current-document path.
    routeMarkdownChangeToTab({
      content,
      documentRevision: options?.documentRevision,
      handleMarkdownTabChange,
      surface: "visual",
      tabId
    });
  }, [
    activeEditorSurface,
    handleMarkdownTabChange,
    isApplyingSourceToVisualSync,
    mainEditorReadOnly,
    sourceMode,
    splitMode
  ]);
  const handleSourceMarkdownTabChange = useCallback((
    tabId: string,
    content: string,
    options?: { documentRevision?: number }
  ) => {
    if (mainEditorReadOnly) return;
    if (
      content !== document.content &&
      (options?.documentRevision === undefined || options.documentRevision === document.revision)
    ) {
      sourceEditSequenceRef.current += 1;
    }

    markSourceEditForHistory(content, options);
    if (splitMode) setActiveEditorSurface("source");
    routeMarkdownChangeToTab({
      content,
      documentRevision: options?.documentRevision,
      handleMarkdownTabChange,
      surface: "source",
      tabId
    });
  }, [
    document.content,
    document.revision,
    handleMarkdownTabChange,
    markSourceEditForHistory,
    mainEditorReadOnly,
    splitMode
  ]);
  const handleSourceMarkdownChange = useCallback((
    content: string,
    options?: { documentRevision?: number }
  ) => {
    if (!activeTabId) return;

    handleSourceMarkdownTabChange(activeTabId, content, options);
  }, [activeTabId, handleSourceMarkdownTabChange]);
  const syncSplitPaneScrollPosition = useCallback((sourceSurface: EditorSurface, sourceElement: HTMLElement) => {
    if (!splitMode) return false;

    if (splitScrollSyncTargetRef.current === sourceSurface) {
      splitScrollSyncTargetRef.current = null;
      return false;
    }

    const targetSurface: EditorSurface = sourceSurface === "source" ? "visual" : "source";
    const targetElement = targetSurface === "visual" ? visualScrollRef.current : sourceScrollRef.current;
    if (!targetElement) return false;

    const sourceMaxScrollTop = Math.max(0, sourceElement.scrollHeight - sourceElement.clientHeight);
    const targetMaxScrollTop = Math.max(0, targetElement.scrollHeight - targetElement.clientHeight);
    if (targetMaxScrollTop <= 0) return true;

    const nextScrollTop =
      sourceMaxScrollTop <= 0
        ? 0
        : Math.round((sourceElement.scrollTop / sourceMaxScrollTop) * targetMaxScrollTop);
    if (Math.abs(targetElement.scrollTop - nextScrollTop) >= 1) {
      splitScrollSyncTargetRef.current = targetSurface;
      targetElement.scrollTop = nextScrollTop;
      window.setTimeout(() => {
        if (splitScrollSyncTargetRef.current === targetSurface) {
          splitScrollSyncTargetRef.current = null;
        }
      }, 0);
    }

    return true;
  }, [splitMode]);
  const scheduleSplitPaneScrollResync = useCallback((sourceSurface: EditorSurface, sourceElement: HTMLElement) => {
    if (!splitMode) return;

    if (splitScrollSyncFrameRef.current !== null) {
      window.cancelAnimationFrame(splitScrollSyncFrameRef.current);
    }

    splitScrollSyncFrameRef.current = window.requestAnimationFrame(() => {
      splitScrollSyncFrameRef.current = null;
      if (!sourceElement.isConnected) return;

      syncSplitPaneScrollPosition(sourceSurface, sourceElement);
    });
  }, [splitMode, syncSplitPaneScrollPosition]);
  const syncSplitPaneScroll = useCallback((sourceSurface: EditorSurface, event: ReactUIEvent<HTMLElement>) => {
    const sourceElement = event.currentTarget;
    if (!syncSplitPaneScrollPosition(sourceSurface, sourceElement)) return;

    scheduleSplitPaneScrollResync(sourceSurface, sourceElement);
  }, [scheduleSplitPaneScrollResync, syncSplitPaneScrollPosition]);
  const handleSourcePaneScroll = useCallback((event: ReactUIEvent<HTMLElement>) => {
    saveDocumentTabViewState(activeTabId, { sourceScrollTop: event.currentTarget.scrollTop });
    syncSplitPaneScroll("source", event);
  }, [activeTabId, saveDocumentTabViewState, syncSplitPaneScroll]);
  const handleVisualPaneScroll = useCallback((event: ReactUIEvent<HTMLElement>) => {
    saveDocumentTabViewState(activeTabId, { visualScrollTop: event.currentTarget.scrollTop });
    syncSplitPaneScroll("visual", event);
  }, [activeTabId, saveDocumentTabViewState, syncSplitPaneScroll]);
  const syncVisualMarkdownAfterEditorCommand = useCallback(() => {
    if (mainEditorReadOnly || !splitMode) return;

    const content = getEditorCurrentMarkdown(document.content);
    setActiveEditorSurface("visual");
    handleMarkdownChange(content, {
      documentRevision: document.revision,
      surface: "visual"
    });
  }, [document.content, document.revision, getEditorCurrentMarkdown, handleMarkdownChange, mainEditorReadOnly, splitMode]);
  const handleImportLocalImages = useCallback(async () => {
    if (mainEditorReadOnly || !hasOpenDocument || activeImageFile || sourceMode) return;

    const images = await openNativeLocalImages({
      title: translate("menu.importLocalImages")
    }).catch(() => null);
    if (!images) {
      showAppToast({
        message: translate("app.clipboardImageSaveFailed"),
        status: "error"
      });
      return;
    }
    if (images.length === 0) return;

    let savedImages: Array<{ alt: string; src: string }>;
    if (compactMode.trueMobile) {
      const batch = await (async () => {
        const inputs = await Promise.all(images.map(async (image) => ({
          documentPath: document.path,
          fileName: await createImageUploadFileName(
            image,
            editorPreferences.preferences.imageUpload.fileNamePattern,
          ),
          folder: "assets",
          image,
        })));
        return appFiles.saveClipboardImages(inputs);
      })().catch((error) => {
        const description = clipboardImageSaveFailureDescription(error);
        showAppToast({
          ...(description ? { description } : {}),
          message: translate("app.clipboardImageSaveFailed"),
          status: "error",
        });
        return null;
      });
      if (batch === null) return;
      savedImages = batch;
      if (document.path) await refreshMarkdownFileTree(document.path).catch(() => {});
    } else {
      savedImages = (await handleSaveEditorResources(createEditorResourceRequest("import", images)))
        .flatMap((resource) => resource.kind === "image"
          ? [{ alt: resource.alt, src: resource.src }]
          : []);
    }
    if (savedImages.length === 0 || savedImages.length !== images.length) return;

    insertEditorMarkdownImages(savedImages);
    syncVisualMarkdownAfterEditorCommand();
  }, [
    activeImageFile,
    appFiles,
    compactMode.trueMobile,
    document.path,
    editorPreferences.preferences.imageUpload.fileNamePattern,
    hasOpenDocument,
    handleSaveEditorResources,
    insertEditorMarkdownImages,
    mainEditorReadOnly,
    refreshMarkdownFileTree,
    sourceMode,
    syncVisualMarkdownAfterEditorCommand,
    translate
  ]);
  const handleImportLocalFiles = useCallback(async () => {
    if (mainEditorReadOnly || !hasOpenDocument || activeImageFile || sourceMode) return;

    const files = await openNativeLocalFiles({
      title: translate("menu.importLocalFiles")
    }).catch(() => null);
    if (!files) {
      showAppToast({
        message: translate("app.clipboardAttachmentSaveFailed"),
        status: "error"
      });
      return;
    }

    const editorFiles = files.map(editorFileForNativeLocalFile);
    const savedAttachments = (await handleSaveEditorResources(createEditorResourceRequest("import", editorFiles)))
      .flatMap((resource) => resource.kind === "attachment"
        ? [{ href: resource.src, label: resource.label }]
        : []);
    if (savedAttachments.length === 0) return;

    insertEditorMarkdownLinks(savedAttachments);
    syncVisualMarkdownAfterEditorCommand();
  }, [
    activeImageFile,
    hasOpenDocument,
    handleSaveEditorResources,
    insertEditorMarkdownLinks,
    mainEditorReadOnly,
    sourceMode,
    syncVisualMarkdownAfterEditorCommand,
    translate
  ]);
  const handleDroppedLocalImage = useCallback(async (target: Extract<NativeMarkdownDroppedTarget, { kind: "image" }>) => {
    if (mainEditorReadOnly || activeImageFile || !document.path) return;

    const payload = markdownImageDragPayloadForFile({
      name: target.name,
      path: target.path,
      relativePath: target.path
    });
    const imageReference = {
      alt: payload.alt,
      src: markdownImageDragSrcForDocument(payload, document.path)
    };

    const insertedAtDropPoint = target.point
      ? insertEditorMarkdownImagesAtPoint([imageReference], target.point)
      : false;
    if (!insertedAtDropPoint) {
      insertEditorMarkdownImages([imageReference]);
    }

    syncVisualMarkdownAfterEditorCommand();
  }, [
    activeImageFile,
    document.path,
    insertEditorMarkdownImages,
    insertEditorMarkdownImagesAtPoint,
    mainEditorReadOnly,
    syncVisualMarkdownAfterEditorCommand
  ]);
  const handleInsertFileTreeImageAsset = useCallback((
    file: NativeMarkdownFolderFile,
    point: { left: number; top: number }
  ) => {
    if (mainEditorReadOnly || activeImageFile || !document.path || file.kind !== "asset") return;

    const payload = markdownImageDragPayloadForFile(file);
    const inserted = insertEditorMarkdownImagesAtPoint(
      [{
        alt: payload.alt,
        src: markdownImageDragSrcForDocument(payload, document.path)
      }],
      point
    );
    if (inserted) syncVisualMarkdownAfterEditorCommand();
  }, [
    activeImageFile,
    document.path,
    insertEditorMarkdownImagesAtPoint,
    mainEditorReadOnly,
    syncVisualMarkdownAfterEditorCommand
  ]);
  const handleNativeMarkdownDrop = useCallback(async (target: NativeMarkdownDroppedTarget) => {
    if (target.kind === "image") {
      await handleDroppedLocalImage(target);
      return;
    }

    await handleDroppedMarkdownPath(target);
  }, [handleDroppedLocalImage, handleDroppedMarkdownPath]);
  const handleInsertMarkdownSnippet = useCallback((...args: Parameters<typeof insertEditorMarkdownSnippet>) => {
    if (activeEditorReadOnly) return;

    insertEditorMarkdownSnippet(...args);
    syncVisualMarkdownAfterEditorCommand();
  }, [activeEditorReadOnly, insertEditorMarkdownSnippet, syncVisualMarkdownAfterEditorCommand]);
  const handleInsertMarkdownImage = useCallback(() => {
    if (activeEditorReadOnly) return;

    insertEditorMarkdownImage();
    syncVisualMarkdownAfterEditorCommand();
  }, [activeEditorReadOnly, insertEditorMarkdownImage, syncVisualMarkdownAfterEditorCommand]);
  const handleInsertMarkdownLink = useCallback(() => {
    runEditorLinkCommand({
      insertMarkdownLink: insertEditorMarkdownLink,
      readOnlyMode: activeEditorReadOnly,
      syncSelectionToolbarFormattingState,
      syncVisualMarkdownAfterEditorCommand
    });
  }, [
    insertEditorMarkdownLink,
    activeEditorReadOnly,
    syncSelectionToolbarFormattingState,
    syncVisualMarkdownAfterEditorCommand
  ]);
  const handleInsertMarkdownTable = useCallback(() => {
    if (activeEditorReadOnly) return;

    insertEditorMarkdownTable();
    syncVisualMarkdownAfterEditorCommand();
  }, [activeEditorReadOnly, insertEditorMarkdownTable, syncVisualMarkdownAfterEditorCommand]);
  const handleRunEditorShortcut = useCallback((...args: Parameters<typeof runEditorShortcut>) => {
    if (activeEditorReadOnly) return false;

    const handled = runEditorShortcut(...args);
    syncVisualMarkdownAfterEditorCommand();
    return handled;
  }, [activeEditorReadOnly, runEditorShortcut, syncVisualMarkdownAfterEditorCommand]);
  const handleCompactFormattingAction = useCallback((action: MarkdownFormattingShortcutAction) => {
    const normalizedShortcuts = normalizeMarkdownShortcuts(editorPreferences.preferences.markdownShortcuts);
    const shortcut = markdownShortcutToKeyboardEventInit(normalizedShortcuts[action]);
    if (!shortcut) return false;

    return handleRunEditorShortcut(shortcut.key, {
      altKey: Boolean(shortcut.altKey),
      code: shortcut.code,
      shiftKey: Boolean(shortcut.shiftKey)
    });
  }, [editorPreferences.preferences.markdownShortcuts, handleRunEditorShortcut]);
  const handleToggleEditorTaskList = useCallback(() => {
    if (activeEditorReadOnly) return false;

    const handled = toggleEditorTaskList();
    if (handled) syncVisualMarkdownAfterEditorCommand();
    return handled;
  }, [activeEditorReadOnly, syncVisualMarkdownAfterEditorCommand, toggleEditorTaskList]);
  const handleSelectionToolbarFormattingAction = useCallback((action: SelectionFormattingToolbarAction) => {
    if (activeEditorReadOnly) return;

    if (!runEditorSelectionFormattingAction(action)) return;

    syncVisualMarkdownAfterEditorCommand();
    syncSelectionToolbarFormattingState();
  }, [
    activeEditorReadOnly,
    runEditorSelectionFormattingAction,
    syncSelectionToolbarFormattingState,
    syncVisualMarkdownAfterEditorCommand
  ]);
  const handleSelectionToolbarHeadingLevelAction = useCallback((level: SelectionHeadingLevel) => {
    if (activeEditorReadOnly) return;
    if (!setEditorSelectionHeadingLevel(level)) return;

    syncVisualMarkdownAfterEditorCommand();
    syncSelectionToolbarFormattingState();
  }, [
    activeEditorReadOnly,
    setEditorSelectionHeadingLevel,
    syncSelectionToolbarFormattingState,
    syncVisualMarkdownAfterEditorCommand
  ]);
  const handleSelectionToolbarInsertLink = useCallback(() => {
    if (activeEditorReadOnly) return;

    setSelectionToolbarAnchor(null);
    handleInsertMarkdownLink();
  }, [activeEditorReadOnly, handleInsertMarkdownLink]);
  const handleSelectionToolbarCopySelection = useCallback(() => {
    const selectedText = activeTextSelection?.text ?? "";
    if (!selectedText.trim() || !navigator.clipboard) return;

    navigator.clipboard.writeText(selectedText)
      .then(() => {
        if (selectionToolbarCopySuccessTimerRef.current !== null) {
          window.clearTimeout(selectionToolbarCopySuccessTimerRef.current);
        }

        setSelectionToolbarCopySucceeded(true);
        selectionToolbarCopySuccessTimerRef.current = window.setTimeout(() => {
          selectionToolbarCopySuccessTimerRef.current = null;
          setSelectionToolbarCopySucceeded(false);
        }, selectionToolbarCopySuccessMs);
      })
      .catch(() => {});
  }, [activeTextSelection]);
  const handleDocumentSearchOpen = useCallback(() => {
    openDocumentSearch();
  }, [openDocumentSearch]);
  const handleDocumentReplaceOpen = useCallback(() => {
    openDocumentReplace();
  }, [openDocumentReplace]);
  const handleDocumentSearchClose = useCallback(() => {
    closeDocumentSearch();
  }, [closeDocumentSearch]);
  const handleDocumentSearchQueryChange = useCallback((query: string) => {
    setDocumentSearchQuery(query);
  }, [setDocumentSearchQuery]);
  const handleDocumentSearchCaseSensitiveChange = useCallback((caseSensitive: boolean) => {
    setDocumentSearchCaseSensitive(caseSensitive);
  }, [setDocumentSearchCaseSensitive]);
  const handleDocumentSearchNext = useCallback(() => {
    navigateDocumentSearch(1);
  }, [navigateDocumentSearch]);
  const handleDocumentSearchPrevious = useCallback(() => {
    navigateDocumentSearch(-1);
  }, [navigateDocumentSearch]);
  const handleDocumentReplace = useCallback(() => {
    if (mainEditorReadOnly || !activeDocumentSearchMatch) return;

    if (documentSearchSurface === "source") {
      handleSourceMarkdownChange(replaceTextRange(document.content, activeDocumentSearchMatch, documentSearchReplacement));
      return;
    }

    replaceEditorSearchMatch(activeDocumentSearchMatch, documentSearchReplacement);
  }, [
    activeDocumentSearchMatch,
    document.content,
    documentSearchReplacement,
    documentSearchSurface,
    handleSourceMarkdownChange,
    replaceEditorSearchMatch,
    mainEditorReadOnly
  ]);
  const handleDocumentReplaceAll = useCallback(() => {
    if (mainEditorReadOnly || documentSearchMatches.length === 0) return;

    if (documentSearchSurface === "source") {
      handleSourceMarkdownChange(replaceTextRanges(document.content, documentSearchMatches, documentSearchReplacement));
      resetDocumentSearchActiveIndex();
      return;
    }

    replaceAllEditorSearchMatches(documentSearchMatches, documentSearchReplacement);
    resetDocumentSearchActiveIndex();
  }, [
    document.content,
    documentSearchMatches,
    documentSearchReplacement,
    documentSearchSurface,
    handleSourceMarkdownChange,
    replaceAllEditorSearchMatches,
    mainEditorReadOnly,
    resetDocumentSearchActiveIndex
  ]);
  useEffect(() => {
    if (!documentSearchOpen || !documentSearchAvailable || documentSearchSurface !== "visual") {
      setVisualDocumentSearchMatches([]);
      return;
    }

    setVisualDocumentSearchMatches(
      findEditorSearchMatches(documentSearchQuery, {
        caseSensitive: documentSearchCaseSensitive
      })
    );
  }, [
    document.content,
    document.revision,
    documentSearchAvailable,
    documentSearchCaseSensitive,
    documentSearchOpen,
    documentSearchQuery,
    documentSearchSurface,
    findEditorSearchMatches,
    visualEditorReadySequence
  ]);
  useEffect(() => {
    if (!documentSearchOpen || documentSearchSurface !== "visual") {
      showEditorSearchMatches([], -1, { suppressEditorChrome: false });
      return;
    }

    showEditorSearchMatches(visualDocumentSearchMatches, normalizedDocumentSearchActiveIndex, {
      suppressEditorChrome: true
    });
  }, [
    documentSearchOpen,
    documentSearchSurface,
    normalizedDocumentSearchActiveIndex,
    showEditorSearchMatches,
    visualDocumentSearchMatches
  ]);
  useEffect(() => {
    if (!documentSearchOpen || documentSearchSurface !== "visual") return;
    if (documentSearchRevealRevision === lastDocumentSearchRevealRevisionRef.current) return;

    lastDocumentSearchRevealRevisionRef.current = documentSearchRevealRevision;
    revealEditorSearchMatch(activeDocumentSearchMatch);
  }, [
    activeDocumentSearchMatch,
    documentSearchOpen,
    documentSearchRevealRevision,
    documentSearchSurface,
    revealEditorSearchMatch
  ]);
  useEffect(() => {
    exportContextRef.current = {
      activeImageFile: Boolean(activeImageFile),
      content: document.content,
      hasOpenDocument,
      name: document.name || "Untitled.md",
      path: document.path
    };
  }, [activeImageFile, document.content, document.name, document.path, hasOpenDocument]);
  const commitActiveVisualMarkdown = useCallback(() => {
    if (!activeTabId) return;
    if (editorMode === "source") return;
    if (editorMode === "split" && activeEditorSurface === "source") return;

    const activeVisualEditor = mainVisualEditorsRef.current.get(activeTabId);
    if (!activeVisualEditor) return;

    handleVisualMarkdownTabChange(
      activeTabId,
      getMarkdownFromEditor(activeVisualEditor, document.content),
      { documentRevision: document.revision }
    );
  }, [
    activeEditorSurface,
    activeTabId,
    document.content,
    document.revision,
    editorMode,
    getMarkdownFromEditor,
    handleVisualMarkdownTabChange
  ]);
  const handleEditorModeSelect = useCallback((nextMode: EditorMode) => {
    if (!sourceModeAvailable) return;
    if (nextMode === editorMode) return;

    captureActiveDocumentViewState();
    // IME changes can still be pending in the visual surface when source mode
    // mounts, so snapshot the originating editor before changing surfaces.
    commitActiveVisualMarkdown();

    if (nextMode === "visual") {
      if (sourceMode) syncSourceEditsToVisualHistory();
      queueEditorModeScroll("visual");
      setEditorMode("visual");
      setActiveEditorSurface("visual");
      return;
    }

    if (nextMode === "source") {
      clearActiveTextSelection();
      queueEditorModeScroll("source");
      setEditorMode("source");
      setActiveEditorSurface("source");
      return;
    }

    clearActiveTextSelection();
    if (sideDocumentGroup) clearSideDocumentGroup();
    setEditorMode("split");
    setActiveEditorSurface(sourceMode ? "source" : "visual");
  }, [
    captureActiveDocumentViewState,
    clearActiveTextSelection,
    clearSideDocumentGroup,
    commitActiveVisualMarkdown,
    editorMode,
    queueEditorModeScroll,
    sideDocumentGroup,
    sourceMode,
    sourceModeAvailable,
    syncSourceEditsToVisualHistory
  ]);
  const handleEditorModeToggle = useCallback(() => {
    handleEditorModeSelect(sourceMode ? "visual" : "source");
  }, [handleEditorModeSelect, sourceMode]);
  const handleEditorSplitToggle = useCallback(() => {
    handleEditorModeSelect(splitMode ? "visual" : "split");
  }, [handleEditorModeSelect, splitMode]);
  const handleOpenMarkdownFolder = useCallback(async () => {
    if (!canChooseLocalWorkspace) return;
    if (compactMode.trueMobile) {
      await openMobileNotebookDialog();
      return;
    }
    if (primaryWindowOwner) {
      await notebookSwitch.switchDesktopNotebook();
      return;
    }
    await requestPrimaryNotebookSwitch({ source: "file-menu" });
  }, [
    canChooseLocalWorkspace,
    compactMode.trueMobile,
    notebookSwitch.switchDesktopNotebook,
    openMobileNotebookDialog,
    primaryWindowOwner
  ]);
  const handleOpenContainingFolder = useCallback((path: string) => {
    openNativeContainingFolder(path).catch(() => {});
  }, []);
  const clearExportSnapshot = useCallback((id: number) => {
    setExportSnapshot((current) => current?.id === id ? null : current);
  }, []);
  const beginDocumentExport = useCallback((kind: MarkdownExportSnapshot["kind"]) => {
    if (!exportFeatureEnabled) return;

    const context = exportContextRef.current;
    if (!context.hasOpenDocument || context.activeImageFile) return;

    exportRequestIdRef.current += 1;
    setExportSnapshot({
      id: exportRequestIdRef.current,
      kind,
      markdown: readCurrentMarkdownForDocument(context.content),
      title: context.name
    });
  }, [exportFeatureEnabled, readCurrentMarkdownForDocument]);
  const handleRenderedExport = useCallback((exported: RenderedMarkdownExport) => {
    if (!exportFeatureEnabled || exportSnapshot?.id !== exported.id) return;

    const pdfSettings = exported.kind === "pdf" ? exportSettings.settings : null;
    const contents = buildMarkdownHtmlDocument({
      bodyHtml: exported.bodyHtml,
      fontFamily: exportSettings.settings.fontFamily,
      language: appLanguage.language,
      pdfAuthor: pdfSettings?.pdfAuthor,
      pdfFooter: pdfSettings?.pdfFooter,
      pdfHeader: pdfSettings?.pdfHeader,
      pdfHeightMm: pdfSettings?.pdfHeightMm,
      pdfMarginMm: pdfSettings?.pdfMarginMm,
      pdfPageBreakOnH1: pdfSettings?.pdfPageBreakOnH1,
      pdfWidthMm: pdfSettings?.pdfWidthMm,
      title: exported.title
    });
    const suggestedName = exportDocumentFileName(exported.title, exported.kind);

    if (exported.kind === "html") {
      saveNativeHtmlFile({
        contents,
        suggestedName
      }).catch(() => {}).finally(() => {
        clearExportSnapshot(exported.id);
      });
      return;
    }

    saveNativePdfFile({
      contents,
      suggestedName
    }).catch(() => {}).finally(() => {
      clearExportSnapshot(exported.id);
    });
  }, [appLanguage.language, clearExportSnapshot, exportFeatureEnabled, exportSettings.settings, exportSnapshot?.id]);
  const exportHtmlDocument = useCallback(() => beginDocumentExport("html"), [beginDocumentExport]);
  const exportPdfDocument = useCallback(() => beginDocumentExport("pdf"), [beginDocumentExport]);
  const exportPandocDocument = useCallback((format: NativePandocExportFormat) => {
    if (!exportFeatureEnabled || !pandocFeatureEnabled) return;

    const context = exportContextRef.current;
    if (!context.hasOpenDocument || context.activeImageFile) return;

    saveNativePandocFile({
      documentPath: context.path,
      format,
      markdown: readCurrentMarkdownForDocument(context.content),
      pandocArgs: exportSettings.settings.pandocArgs,
      pandocPath: exportSettings.settings.pandocPath,
      suggestedName: exportDocumentFileName(context.name, format)
    }).catch((error: unknown) => {
      if (!isPandocSetupError(error)) {
        showAppToast({
          message: translate("app.pandocExportFailed"),
          status: "error"
        });
        return;
      }

      showNativePandocSetup({
        cancelLabel: translate("app.cancelPandocSetup"),
        installLabel: translate("app.installPandoc"),
        message: translate("app.pandocRequiredMessage"),
        setPathLabel: translate("app.setPandocPath"),
        title: translate("app.pandocRequiredTitle")
      })
        .then((action) => {
          const settingsProjectRoot = blankWorkspace ? null : fileTree.settingsProjectRoot;
          if (action === "setPath") {
            return openSettingsModal(
              "exportPandocPath",
              settingsProjectRoot,
              fileTreeSourcePath ?? exportContextRef.current.path ?? settingsProjectRoot
            );
          }
          return runPandocSetupAction(action, settingsProjectRoot);
        })
        .catch(() => {});
    });
  }, [
    exportFeatureEnabled,
    exportSettings.settings.pandocArgs,
    exportSettings.settings.pandocPath,
    pandocFeatureEnabled,
    blankWorkspace,
    fileTree.settingsProjectRoot,
    fileTreeSourcePath,
    openSettingsModal,
    readCurrentMarkdownForDocument,
    translate
  ]);
  const exportDocxDocument = useCallback(() => exportPandocDocument("docx"), [exportPandocDocument]);
  const exportEpubDocument = useCallback(() => exportPandocDocument("epub"), [exportPandocDocument]);
  const exportLatexDocument = useCallback(() => exportPandocDocument("latex"), [exportPandocDocument]);
  useEffect(() => {
    if (sourceModeAvailable) return;

    setEditorMode("visual");
    setActiveEditorSurface("visual");
  }, [sourceModeAvailable]);
  useEffect(() => {
    if (activeImageFile || !activeTabId || !hasOpenDocument) return;

    const viewState = documentTabViewStatesRef.current.get(activeTabId);
    const restoreFrame = window.requestAnimationFrame(() => {
      if ((editorMode === "visual" || editorMode === "split") && visualScrollRef.current) {
        const pendingScroll = pendingEditorModeScrollRef.current;
        const visualScrollTop = pendingScroll?.tabId === activeTabId && pendingScroll.targetSurface === "visual"
          ? pendingScroll.progress * Math.max(0, visualScrollRef.current.scrollHeight - visualScrollRef.current.clientHeight)
          : viewState?.visualScrollTop ?? 0;
        restoreElementScrollTop(visualScrollRef.current, visualScrollTop);
        saveDocumentTabViewState(activeTabId, { visualScrollTop });
      }

      if ((editorMode === "source" || editorMode === "split") && sourceScrollRef.current) {
        const pendingScroll = pendingEditorModeScrollRef.current;
        const sourceScrollTop = pendingScroll?.tabId === activeTabId && pendingScroll.targetSurface === "source"
          ? pendingScroll.progress * Math.max(0, sourceScrollRef.current.scrollHeight - sourceScrollRef.current.clientHeight)
          : viewState?.sourceScrollTop ?? 0;
        restoreElementScrollTop(sourceScrollRef.current, sourceScrollTop);
        saveDocumentTabViewState(activeTabId, { sourceScrollTop });
      }

      const pendingScroll = pendingEditorModeScrollRef.current;
      if (pendingScroll?.tabId === activeTabId && (
        pendingScroll.targetSurface === editorMode
        || editorMode === "split"
      )) {
        pendingEditorModeScrollRef.current = null;
      }
    });

    return () => {
      window.cancelAnimationFrame(restoreFrame);
    };
  }, [
    activeImageFile,
    activeTabId,
    document.revision,
    editorMode,
    hasOpenDocument,
    saveDocumentTabViewState,
    visualEditorReadySequence
  ]);
  const appUpdater = useAutoUpdater(appLanguage.language, updaterFeatureEnabled && appLanguage.ready && !editorPreferences.loading, {
    autoCheck: updaterFeatureEnabled && editorPreferences.preferences.autoUpdateEnabled,
    beforeRestart: saveDirtyMarkdownFiles,
    confirmRestart: confirmCanDiscardCurrentDocument,
    currentVersion: appVersion
  });
  const nativeMenuHandlers = useNativeMenuHandlers({
    checkForUpdates: updaterFeatureEnabled ? appUpdater.checkForUpdates : undefined,
    closeDocument: handleCloseCurrentFile,
    exportDocx: exportFeatureEnabled && pandocFeatureEnabled ? exportDocxDocument : undefined,
    exportEpub: exportFeatureEnabled && pandocFeatureEnabled ? exportEpubDocument : undefined,
    exportHtml: exportFeatureEnabled ? exportHtmlDocument : undefined,
    exportLatex: exportFeatureEnabled && pandocFeatureEnabled ? exportLatexDocument : undefined,
    exportPdf: exportFeatureEnabled ? exportPdfDocument : undefined,
    importLocalFiles: handleImportLocalFiles,
    importLocalImages: handleImportLocalImages,
    insertMarkdownImage: handleInsertMarkdownImage,
    insertMarkdownLink: handleInsertMarkdownLink,
    insertMarkdownSnippet: handleInsertMarkdownSnippet,
    insertMarkdownTable: handleInsertMarkdownTable,
    language: appLanguage.language,
    markdownShortcuts: editorPreferences.preferences.markdownShortcuts,
    openDocument: handleOpenMarkdownFile,
    openRecentFile: handleOpenRecentMarkdownFile,
    clearRecentFiles: clearRecentMarkdownFiles,
    openFolder: canChooseLocalWorkspace ? handleOpenMarkdownFolder : undefined,
    openQuickOpen: handleQuickOpenOpen,
    openSettings: handleOpenSettings,
    quitApplication: requestAppExit,
    runEditorShortcut: handleRunEditorShortcut,
    saveDocument: handleSaveDocument,
    saveDocumentAs,
    syncNow: runApplicationSyncNow,
    toggleDocumentHistory: handleDocumentHistoryOpen,
    toggleFullscreen: toggleNativeWindowFullscreen,
    toggleMarkdownFiles: handleFileTreeToggle,
    toggleReadOnlyMode: handleReadOnlyModeToggle,
    toggleSourceMode: handleEditorModeToggle
  });

  useNativeMarkdownDrop(
    handleNativeMarkdownDrop,
    appFeatures.fileDrop && settingsModalRequest === null
  );
  useNativeMenus(nativeMenuHandlers, appLanguage.ready ? appLanguage.language : null, {
    enabled: appFeatures.applicationMenu && settingsModalRequest === null,
    markdownShortcuts: editorPreferences.preferences.markdownShortcuts,
    recentFiles: recentMarkdownFiles
  });
  useApplicationShortcuts({
    enabled: appFeatures.applicationShortcuts && settingsModalRequest === null,
    closeDocument: handleCloseCurrentFile,
    exportHtml: exportFeatureEnabled ? exportHtmlDocument : undefined,
    exportPdf: exportFeatureEnabled ? exportPdfDocument : undefined,
    markdownShortcuts: editorPreferences.preferences.markdownShortcuts,
    openBlankEditorWindow: windowsSelfDrawnChromeEnabled ? handleOpenBlankEditorWindow : undefined,
    openDocument: handleOpenMarkdownFile,
    openDocumentReplace: handleDocumentReplaceOpen,
    openDocumentSearch: handleDocumentSearchOpen,
    openSettings: handleOpenSettings,
    openWorkspaceSearch: handleGlobalSearchOpen,
    openFolder: canChooseLocalWorkspace ? handleOpenMarkdownFolder : undefined,
    openQuickOpen: handleQuickOpenOpen,
    platform: desktopPlatform,
    saveDocument: handleSaveDocument,
    saveDocumentAs,
    syncNow: runApplicationSyncNow,
    toggleDocumentHistory: handleDocumentHistoryOpen,
    toggleMarkdownFiles: handleFileTreeToggle,
    toggleReadOnlyMode: handleReadOnlyModeToggle,
    toggleSourceMode: handleEditorModeToggle,
    toggleTypewriterMode: handleTypewriterModeToggle,
    toggleVimMode: handleVimModeToggle,
    toggleViewMode: handleViewModeCycle
  });

  const quickOpenFilePaths = useMemo(
    () => [
      ...documentTabs.flatMap((tab) => tab.path ? [tab.path] : []),
      ...imageTabs.map((tab) => tab.path)
    ],
    [documentTabs, imageTabs]
  );

  const handleOpenTitlebarTabToSide = useCallback((tabId: string, primaryTabId = activeTabId ?? undefined) => {
    if (!primaryTabId || tabId === primaryTabId) return;
    const tab = documentTabs.find((candidate) => candidate.id === tabId);
    const primaryTab = documentTabs.find((candidate) => candidate.id === primaryTabId);
    if (!tab?.path || !primaryTab?.path) return;

    captureActiveDocumentViewState();
    setActiveImageFile(null);
    clearActiveTextSelection();
    if (splitMode) {
      setEditorMode("visual");
      setActiveEditorSurface("visual");
    }
    if (primaryTabId !== activeTabId) selectMarkdownTab(primaryTabId);
    openSideDocumentGroup({
      primaryFilePath: primaryTab.path,
      primaryTabId,
      sideFilePath: tab.path,
      sideTabId: tabId
    });
  }, [
    activeTabId,
    captureActiveDocumentViewState,
    clearActiveTextSelection,
    documentTabs,
    openSideDocumentGroup,
    selectMarkdownTab,
    splitMode
  ]);
  const handleCancelTitlebarSideBySide = useCallback((tabId: string) => {
    if (!sideDocumentGroup) return;
    if (sideDocumentGroup.primaryTabId !== tabId && sideDocumentGroup.sideTabId !== tabId) return;

    clearSideDocumentGroup();
  }, [clearSideDocumentGroup, sideDocumentGroup]);
  const draggedMarkdownTabIdFromEvent = useCallback((event: ReactDragEvent<HTMLElement>) => {
    return event.dataTransfer.getData(markdownTabDragDataType);
  }, []);
  const canDropMarkdownTabOnEditor = useCallback((tabId: string) => {
    if (!editorPreferences.preferences.showDocumentTabs || activeImageFile || !hasOpenDocument || !activeTabId) return false;
    if (!tabId || tabId === activeTabId || tabId === sideDocumentGroup?.sideTabId) return false;

    const draggedTab = documentTabs.find((tab) => tab.id === tabId);
    const activeTab = documentTabs.find((tab) => tab.id === activeTabId);
    return Boolean(draggedTab?.path && activeTab?.path);
  }, [
    activeImageFile,
    activeTabId,
    documentTabs,
    editorPreferences.preferences.showDocumentTabs,
    hasOpenDocument,
    sideDocumentGroup?.sideTabId
  ]);
  const handleEditorContentDragOver = useCallback((event: ReactDragEvent<HTMLDivElement>) => {
    const draggedTabId = draggedMarkdownTabIdFromEvent(event);
    if (!canDropMarkdownTabOnEditor(draggedTabId)) return;

    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setEditorTabDropTargetActive(true);
  }, [canDropMarkdownTabOnEditor, draggedMarkdownTabIdFromEvent]);
  const handleEditorContentDragLeave = useCallback(() => {
    setEditorTabDropTargetActive(false);
  }, []);
  const handleEditorContentDrop = useCallback((event: ReactDragEvent<HTMLDivElement>) => {
    const draggedTabId = draggedMarkdownTabIdFromEvent(event);
    if (!canDropMarkdownTabOnEditor(draggedTabId)) {
      setEditorTabDropTargetActive(false);
      return;
    }

    event.preventDefault();
    setEditorTabDropTargetActive(false);
    handleOpenTitlebarTabToSide(draggedTabId);
  }, [
    canDropMarkdownTabOnEditor,
    draggedMarkdownTabIdFromEvent,
    handleOpenTitlebarTabToSide
  ]);
  const handleSideDocumentChange = useCallback((content: string) => {
    if (!sideDocumentGroup || sideEditorReadOnly) return;

    handleMarkdownTabChange(sideDocumentGroup.sideTabId, content);
  }, [handleMarkdownTabChange, sideDocumentGroup, sideEditorReadOnly]);

  const handleCloseTitlebarTab = useCallback(async (tabId: string) => {
    captureActiveDocumentViewState();

    const imageTab = imageTabs.find((tab) => tab.id === tabId);
    if (imageTab) {
      const closingActiveImage = activeImageFile ? imageDocumentTabId(activeImageFile.path) === tabId : false;
      setImageTabs((currentTabs) => currentTabs.filter((tab) => tab.id !== tabId));
      if (closingActiveImage) setActiveImageFile(null);
      clearActiveTextSelection();
      return true;
    }

    const closed = await closeMarkdownTab(tabId);
    if (!closed) return false;

    if (sideDocumentGroup?.primaryTabId === tabId || sideDocumentGroup?.sideTabId === tabId) {
      clearSideDocumentGroup();
    }
    clearActiveTextSelection();
    return true;
  }, [
    activeImageFile,
    captureActiveDocumentViewState,
    clearActiveTextSelection,
    clearSideDocumentGroup,
    closeMarkdownTab,
    imageTabs,
    sideDocumentGroup
  ]);

  const focusEditorInDocumentPane = useCallback((target: "main" | "side") => {
    if (titlebarTabFocusTimerRef.current !== null) {
      window.clearTimeout(titlebarTabFocusTimerRef.current);
      titlebarTabFocusTimerRef.current = null;
    }

    titlebarTabFocusTimerRef.current = window.setTimeout(() => {
      titlebarTabFocusTimerRef.current = null;

      const pane =
        target === "side"
          ? sideDocumentSurfaceRef.current?.querySelector<HTMLElement>(".side-document-pane") ?? null
          : mainDocumentPaneRef.current;
      if (!pane) return;

      const sourceEditorSelector = ".markdown-source-editor [role='textbox']";
      const visualEditorSelector = ".markdown-paper [role='textbox']";
      const preferredSelector =
        target === "side"
          ? sourceMode ? sourceEditorSelector : visualEditorSelector
          : sourceMode || (splitMode && activeEditorSurface === "source") ? sourceEditorSelector : visualEditorSelector;
      const editor =
        pane.querySelector<HTMLElement>(preferredSelector) ??
        pane.querySelector<HTMLElement>("[role='textbox']");

      editor?.focus({ preventScroll: true });
    }, 0);
  }, [activeEditorSurface, sourceMode, splitMode]);

  useEffect(() => () => {
    if (titlebarTabFocusTimerRef.current !== null) {
      window.clearTimeout(titlebarTabFocusTimerRef.current);
      titlebarTabFocusTimerRef.current = null;
    }
  }, []);

  const handleSelectTitlebarTab = useCallback((tabId: string) => {
    captureActiveDocumentViewState();
    setDocumentOperationTarget("main");

    const imageTab = imageTabs.find((tab) => tab.id === tabId);
    if (imageTab) {
      setActiveImageFile(imageTab);
      clearActiveTextSelection();
      return;
    }

    setActiveImageFile(null);
    clearActiveTextSelection();
    selectMarkdownTab(tabId);
  }, [
    captureActiveDocumentViewState,
    clearActiveTextSelection,
    imageTabs,
    selectMarkdownTab
  ]);
  const handleFocusTitlebarTab = useCallback((tabId: string) => {
    const target = sideDocumentGroup?.sideTabId === tabId ? "side" : "main";
    setDocumentOperationTarget(target);
    focusEditorInDocumentPane(target);
  }, [focusEditorInDocumentPane, sideDocumentGroup?.sideTabId]);
  const handleRenameTitlebarTab = useCallback(async (tab: MarkdownTabsBarDocumentItem, fileName: string) => {
    const file = documentTabAsFolderFile(tab);
    if (!file) return;

    try {
      const renamedFile = await renameMarkdownTreeFile(file, fileName);
      if (renamedFile) applyRenamedTreeFile(file.path, renamedFile);
    } catch (error) {
      showAppToast({
        message: nativeFileOperationFailureMessage(translate("app.markdownFileRenameFailed"), error),
        status: "error"
      });
    }
  }, [applyRenamedTreeFile, renameMarkdownTreeFile, translate]);

  const titlebarDocumentTabs = documentTabsVisible ? (
    <MarkdownTabsBar
      activeTabId={activeTitlebarTabId}
      focusedTabId={focusedSideDocumentTabId ?? activeTitlebarTabId}
      items={titlebarItems}
      language={appLanguage.language}
      nativeDragRegionEnabled={nativeWindowChromeEnabled}
      placement="titlebar"
      onCancelSideBySide={handleCancelTitlebarSideBySide}
      onCloseTab={handleCloseTitlebarTab}
      onFocusTab={handleFocusTitlebarTab}
      onNewTab={() => {
        captureActiveDocumentViewState();
        setActiveImageFile(null);
        createBlankDocument().catch(() => {});
      }}
      onOpenTabToSide={handleOpenTitlebarTabToSide}
      onRevealTabInFileTree={handleRevealPathInFileTree}
      onRenameTab={handleRenameTitlebarTab}
      onSelectTab={handleSelectTitlebarTab}
    />
  ) : null;
  const appTitlebarActions = useMemo(
    () => {
      const availableActions = editorPreferences.preferences.titlebarActions;

      if (!viewModeChrome.titlebarActions) {
        return editorPreferences.preferences.viewMode === "immersive" && viewModeChrome.viewModeToggle
          ? availableActions.filter((action) => action.id === "viewMode")
          : [];
      }

      return viewModeChrome.viewModeToggle
        ? availableActions
        : availableActions.filter((action) => action.id !== "viewMode");
    },
    [
      editorPreferences.preferences.titlebarActions,
      viewModeChrome.titlebarActions,
      viewModeChrome.viewModeToggle
    ]
  );
  const mainVisualEditorTabs = documentTabs.filter((tab) => tab.open);
  const mainVisualEditors = (
    <>
      {mainVisualEditorTabs.map((tab) => {
        const tabActive = tab.id === activeTabId;
        const tabVisualBlocked = shouldBlockLargeMarkdownVisual(tab.content, {
          sizeBytes: tab.sizeBytes
        });
        if (tabVisualBlocked) {
          if (!tabActive || (sourceMode && !compactMode.compact)) return null;

          return (
            <LargeMarkdownNotice
              key={`${tab.id}:large-visual-notice`}
              language={appLanguage.language}
              onOpenSourceMode={compactMode.compact ? undefined : handleEditorModeToggle}
            />
          );
        }

        const visualHidden = !tabActive || (sourceMode && !compactMode.compact);

        return (
          <div
            key={tab.id}
            aria-hidden={visualHidden ? "true" : undefined}
            className="h-full min-h-0"
            hidden={visualHidden}
          >
            <MarkdownPaper
              autoFocus={
                tabActive &&
                (!sourceMode || compactMode.compact) &&
                (splitMode ? activeEditorSurface === "visual" : shouldFocusEditorOnReady(tab.content))
              }
              bottomOverlayInset={tabActive && !sourceMode ? quietStatusOverlayInset : 0}
              bodyFontSize={editorPreferences.preferences.bodyFontSize}
              contentWidth={activeEditorContentWidth}
              contentWidthPx={activeEditorContentWidthPx}
              documentKey={tab.id}
              documentPath={tab.path}
              editorFontFamily={editorPreferences.preferences.editorFontFamily}
              editorTheme={appTheme.editorTheme}
              extendedSyntax={editorPreferences.preferences.extendedSyntax}
              initialContent={tab.content}
              language={appLanguage.language}
              lineHeight={editorPreferences.preferences.lineHeight}
              markdownShortcuts={editorPreferences.preferences.markdownShortcuts}
              paragraphSpacingPx={editorPreferences.preferences.paragraphSpacingPx}
              onActiveOutlineIndexChange={tabActive ? handleActiveOutlineIndexChange : undefined}
              onEditorReady={(readyEditor, disposedEditor) =>
                handleMainVisualEditorReady(tab.id, readyEditor, disposedEditor)
              }
              onMarkdownChange={(content) => handleVisualMarkdownTabChange(
                tab.id,
                content,
                { documentRevision: tab.revision }
              )}
              onContentWidthChange={editorWidthResizerVisible ? handleEditorContentWidthChange : undefined}
              onContentWidthResizeEnd={editorWidthResizerVisible ? handleEditorContentWidthResizeEnd : undefined}
              onSaveEditorResources={(request) => handleSaveEditorResources(request, tab.path)}
              openLocalAttachment={(src) => handleOpenLocalAttachment(src, tab.path)}
              openExternalUrl={handleOpenEditorLink}
              readOnly={editorReadOnlyForPath(readOnlyMode, tab.path, guardedPaths)}
              onTextSelectionChange={tabActive ? handleTextSelectionChange : undefined}
              resolveImageSrc={editorImageSrcResolverForPath(tab.path)}
              revision={tab.revision}
              onScroll={tabActive ? handleVisualPaneScroll : undefined}
              scrollRef={tabActive ? visualScrollRef : undefined}
              hideHeadingMarkersOnFocus={editorPreferences.preferences.hideHeadingMarkersOnFocus}
              tableColumnWidthMode={editorPreferences.preferences.tableColumnWidthMode}
              topInset="titlebar"
              typewriterModeEnabled={editorPreferences.preferences.typewriterModeEnabled}
              vimModeEnabled={editorPreferences.preferences.vimModeEnabled}
              workspaceFiles={fileTreeFiles}
              wrapCodeBlocks={editorPreferences.preferences.wrapCodeBlocks}
            />
          </div>
        );
      })}
    </>
  );
  const compactController = useMemo<CompactAppController>(() => ({
    actions: {
      openDocumentHistory: handleDocumentHistoryOpen,
      openDocumentSearch: handleDocumentSearchOpen,
      runApplicationSyncNow,
      saveDocument: handleSaveDocument
    },
    appearance: {
      activeTheme: appTheme.activeTheme,
      appearanceMode: appTheme.appearanceMode,
      catalog: appTheme.catalog,
      darkTheme: appTheme.darkTheme,
      lightTheme: appTheme.lightTheme,
      selectAppearanceMode: appTheme.selectAppearanceMode,
      selectTheme: appTheme.selectTheme,
      themeError: appTheme.themeError
    },
    capabilities: {
      imageImport: appFeatures.imageImport,
      openLocalAttachments: appFeatures.openLocalAttachments,
      applicationSync: appFeatures.projectSync,
      mcpPolicy: mcpRuntime.policyAvailable,
      systemFonts: appFeatures.systemFonts,
      trueMobile: compactMode.trueMobile
    },
    document: {
      createBlankDocument: handleCreateCompactDocument,
      document,
      saveCurrentDocument
    },
    editor: {
      getSelectionFormattingState: editor.getSelectionFormattingState,
      host: mainVisualEditors,
      importLocalImages: handleImportLocalImages,
      insertMarkdownImage: handleInsertMarkdownImage,
      insertMarkdownLink: handleInsertMarkdownLink,
      readOnly: mainEditorReadOnly,
      runEditorShortcut: handleRunEditorShortcut,
      runFormattingAction: handleCompactFormattingAction,
      setSelectionHeadingLevel: handleSelectionToolbarHeadingLevelAction,
      toggleTaskList: handleToggleEditorTaskList
    },
    files: {
      createFile: createMarkdownTreeFile,
      createFolder: createMarkdownTreeFolder,
      deleteFile: handleDeleteMarkdownTreeFile,
      files: fileTree.files,
      moveFile: handleMoveMarkdownTreeFile,
      openFile: (file) => handleOpenTreeFile(file, { managed: compactMode.trueMobile }),
      openMarkdownFolder: async () => {
        await handleOpenMarkdownFolder();
        return null;
      },
      renameFile: handleRenameCompactFile,
      sourcePath: fileTree.sourcePath
    },
    language: appLanguage.language,
    mcp: mcpRuntime,
    navigationRequest: compactNavigationRequest,
    preferences: {
      loading: editorPreferences.loading,
      preferences: editorPreferences.preferences,
      updatePreferences: handleCompactPreferencesChange
    },
    workspace: {
      openNotebookManager: canChooseLocalWorkspace
        ? compactMode.trueMobile
          ? openMobileNotebookDialog
          : () => notebookSwitch.switchDesktopNotebook()
        : undefined,
      primaryRoot: primaryIntegrationRoot,
      syncConfigDocument: syncConfig.appliedDocument
    },
    saveState: compactSaveState,
    selectLanguage: appLanguage.selectLanguage,
    sync: compactSyncSettings
  }), [
    appFeatures.imageImport,
    appFeatures.openLocalAttachments,
    appFeatures.projectSync,
    appFeatures.systemFonts,
    appLanguage.selectLanguage,
    appTheme.activeTheme,
    appTheme.appearanceMode,
    appTheme.catalog,
    appTheme.darkTheme,
    appTheme.lightTheme,
    appTheme.selectAppearanceMode,
    appTheme.selectTheme,
    appTheme.themeError,
    canChooseLocalWorkspace,
    compactMode.trueMobile,
    compactMode.compact,
    compactSaveState,
    compactSyncSettings,
    document,
    editor.getSelectionFormattingState,
    editorPreferences.loading,
    editorPreferences.preferences,
    handleCompactPreferencesChange,
    handleCreateCompactDocument,
    createMarkdownTreeFile,
    createMarkdownTreeFolder,
    fileTree.files,
    handleOpenMarkdownFolder,
    fileTree.sourcePath,
    handleDocumentHistoryOpen,
    handleDocumentSearchOpen,
    handleSelectionToolbarHeadingLevelAction,
    handleDeleteMarkdownTreeFile,
    handleCompactFormattingAction,
    handleInsertMarkdownImage,
    handleImportLocalImages,
    handleInsertMarkdownLink,
    handleMoveMarkdownTreeFile,
    handleOpenTreeFile,
    handleRenameCompactFile,
    handleSaveDocument,
    handleRunEditorShortcut,
    handleToggleEditorTaskList,
    appLanguage.language,
    mainVisualEditors,
    mcpRuntime,
    compactNavigationRequest,
    notebookSwitch.switchDesktopNotebook,
    openMobileNotebookDialog,
    primaryIntegrationRoot,
    syncConfig.appliedDocument,
    mainEditorReadOnly,
    runApplicationSyncNow,
    saveCurrentDocument
  ]);
  const documentSearchOverlay = documentSearchOpen && documentSearchAvailable ? (
    <DocumentSearchBar
      activeIndex={normalizedDocumentSearchActiveIndex}
      caseSensitive={documentSearchCaseSensitive}
      language={appLanguage.language}
      matchCount={documentSearchMatchCount}
      query={documentSearchQuery}
      readOnly={mainEditorReadOnly}
      replaceOpen={documentSearchReplaceOpen}
      replacement={documentSearchReplacement}
      onCaseSensitiveChange={handleDocumentSearchCaseSensitiveChange}
      onClose={handleDocumentSearchClose}
      onNext={handleDocumentSearchNext}
      onPrevious={handleDocumentSearchPrevious}
      onQueryChange={handleDocumentSearchQueryChange}
      onReplace={handleDocumentReplace}
      onReplaceAll={handleDocumentReplaceAll}
      onReplaceOpenChange={setDocumentSearchReplaceOpen}
      onReplacementChange={setDocumentSearchReplacement}
    />
  ) : null;
  const documentHistoryOverlay = documentHistoryOpen && document.path ? (
    <DocumentHistoryDialog
      documentPath={document.path}
      language={appLanguage.language}
      onClose={() => setDocumentHistoryOpen(false)}
      onRestore={handleDocumentHistoryRestore}
      refreshKey={documentHistoryRefreshKey}
      rightInsetPx={0}
      windowsSelfDrawnChrome={!compactMode.compact && windowsSelfDrawnChromeEnabled}
    />
  ) : null;
  const mobileNotebookDialog = mobileNotebookDialogOpen && compactMode.trueMobile && canChooseLocalWorkspace ? (
    <MobileNotebookDialog
      error={remoteNotebookError}
      language={appLanguage.language}
      loading={remoteNotebookLoading}
      localNames={mobileNotebookLocalNames}
      remoteEntries={remoteNotebookEntries}
      onCancel={closeMobileNotebookDialog}
      onCreate={switchMobileNotebook}
      onRefresh={openMobileNotebookDialog}
      onRestore={restoreMobileNotebook}
      onSwitch={switchMobileNotebook}
    />
  ) : null;
  const desktopRemoteNotebookDialog = remoteNotebookDialogOpen &&
    canChooseLocalWorkspace &&
    primaryWindowOwner &&
    !compactMode.trueMobile ? (
      <RemoteNotebookDialog
        allowCurrentNotebookSelection={remoteNotebookCatalogMode === "select"}
        currentNotebookName={currentDesktopNotebookName}
        entries={remoteNotebookEntries}
        error={remoteNotebookError}
        language={appLanguage.language}
        loading={remoteNotebookLoading}
        onCancel={closeRemoteNotebookDialog}
        onRefresh={refreshDesktopRemoteNotebookCatalog}
        onRestore={selectDesktopRemoteNotebook}
      />
    ) : null;
  const settingsModal = settingsModalRequest && !compactMode.trueMobile ? (
    <Suspense
      fallback={(
        <SettingsWindowLoadingShell
          onClose={closeSettingsModal}
          presentation="modal"
        />
      )}
    >
      <SettingsWindow
        key={settingsModalRequest.generation}
        context={settingsModalRequest.context}
        initialTarget={settingsModalRequest.target}
        onClose={closeSettingsModal}
        presentation="modal"
      />
    </Suspense>
  ) : null;

  if (onboardingVisible && !compactNavigationRequest) {
    return (
      <>
        <AppToaster language={appLanguage.language} />
        <div
          className="contents"
          data-settings-modal-background
          data-settings-project-root={settingsModalRequest?.context.projectRoot ?? undefined}
          data-settings-workspace-source-path={settingsModalRequest?.context.workspaceSourcePath ?? undefined}
          inert={settingsModalRequest ? true : undefined}
        >
          <WelcomeScreen
            error={primaryWorkspace.error}
            formFactor={compactMode.trueMobile ? "mobile" : "desktop"}
            language={appLanguage.language}
            localWorkspaceSelectionAllowed={canChooseLocalWorkspace}
            status={primaryWorkspace.status}
            onChooseDesktopRoot={() => notebookSwitch.switchDesktopNotebook()}
            onCreateMobileRoot={openMobileNotebookDialog}
            onDeferDesktopSetup={primaryWorkspace.deferDesktopSetup}
            onOpenExternalFile={openExternalFileInNewWindow}
            onRestoreFromCloud={
              !compactMode.trueMobile && canChooseLocalWorkspace
                ? openDesktopRemoteNotebookDialog
                : undefined
            }
            onRetry={primaryWorkspace.retry}
          />
          {desktopRemoteNotebookDialog}
          {mobileNotebookDialog}
        </div>
        {settingsModal}
      </>
    );
  }

  return compactMode.compact ? (
    <>
      <AppToaster language={appLanguage.language} />
      <div
        className="contents"
        data-settings-modal-background
        data-settings-project-root={settingsModalRequest?.context.projectRoot ?? undefined}
        data-settings-workspace-source-path={settingsModalRequest?.context.workspaceSourcePath ?? undefined}
        inert={settingsModalRequest ? true : undefined}
      >
        <CompactAppShell
          controller={compactController}
          onNavigationRequestComplete={completeCompactNavigationRequest}
          subscribeToSystemBack={
            compactMode.trueMobile
              ? getAppRuntime().navigation.subscribeToSystemBack
              : undefined
          }
        />
        {documentSearchOverlay}
        {documentHistoryOverlay}
        {desktopRemoteNotebookDialog}
        {mobileNotebookDialog}
      </div>
      {settingsModal}
    </>
  ) : (
    <>
      <AppToaster language={appLanguage.language} />
      <div
        className="contents"
        data-settings-modal-background
        data-settings-project-root={settingsModalRequest?.context.projectRoot ?? undefined}
        data-settings-workspace-source-path={settingsModalRequest?.context.workspaceSourcePath ?? undefined}
        inert={settingsModalRequest ? true : undefined}
      >
        <main className="app-shell group/app relative grid h-full w-full grid-rows-[minmax(0,1fr)] overflow-hidden overscroll-none bg-(--bg-primary) text-(--text-primary)">
        <NativeTitleBar
          compactLayout={compactViewport}
          dirty={!activeImageFile && hasOpenDocument && document.dirty}
          documentKind={titleDocumentKind}
          documentName={titleDocumentName}
          language={appLanguage.language}
          markdownFilesButtonVisible={viewModeChrome.fileTreeButton && viewModeChrome.fileTree && fileTreeContentVisible}
          markdownFilesOpen={visibleFileTreeOpen}
          markdownFilesResizing={fileTreeResizing}
          markdownFilesWidth={fileTreeWidth}
          menuHandlers={nativeMenuHandlers}
          syncNowShortcut={editorPreferences.preferences.markdownShortcuts.syncNow}
          nativeWindowChrome={nativeWindowChromeEnabled}
          openMarkdownButtonVisible={viewModeChrome.openButton && canChooseLocalWorkspace}
          quickCreateMarkdownFileVisible={viewModeChrome.quickCreateButton && !visibleFileTreeOpen}
          historyDisabled={!documentHistoryAvailable}
          saveDisabled={!hasOpenDocument || Boolean(activeImageFile)}
          splitMode={splitMode}
          sourceMode={sourceMode}
          sourceModeDisabled={!sourceModeAvailable}
          theme={appTheme.resolvedTheme}
          titlebarActions={appTitlebarActions}
          titleContent={titlebarDocumentTabs}
          viewMode={editorPreferences.preferences.viewMode}
          onSelectViewMode={handleViewModeSelect}
          onCreateMarkdownFile={handleQuickCreateMarkdownTreeFile}
          onExitApp={handleExitApp}
          onOpenBlankEditorWindow={windowsSelfDrawnChromeEnabled ? handleOpenBlankEditorWindow : undefined}
          onOpenMarkdown={handleOpenMarkdownFile}
          onOpenMarkdownFolder={canChooseLocalWorkspace ? handleOpenMarkdownFolder : undefined}
          onOpenSettings={handleOpenSettings}
          onSaveMarkdown={handleSaveDocument}
          onSelectEditorMode={handleEditorModeSelect}
          onShowDocumentHistory={handleDocumentHistoryOpen}
          onShowAbout={handleShowAbout}
          onTitlebarActionsChange={handleTitlebarActionsChange}
          onToggleMarkdownFiles={handleFileTreeToggle}
          onToggleSplitMode={handleEditorSplitToggle}
          onToggleSourceMode={handleEditorModeToggle}
          onToggleTheme={appTheme.toggleTheme}
          onToggleWindowMaximized={toggleNativeWindowMaximized}
          workspaceName={fileTree.sourcePath ? fileTreeRootName : undefined}
        />

        {documentHistoryOverlay}

        {assetCleanup.dialogOpen ? (
          <AssetCleanupDialog
            error={assetCleanup.error}
            index={assetCleanup.index}
            key={assetCleanup.revision}
            language={appLanguage.language}
            loading={assetCleanup.loading}
            trashing={assetCleanup.trashing}
            onClose={assetCleanup.closeDialog}
            onRefresh={assetCleanup.refresh}
            onTrash={assetCleanup.trashAssets}
          />
        ) : null}

        <span className="screen-reader-title sr-only">{titleDocumentName}</span>

        <WorkspaceLayout
          compactFileTreeOverlay={compactViewport}
          documentSearchAvailable={documentSearchAvailable}
          documentSearchOpen={documentSearchOpen}
          editorDropTargetActive={editorTabDropTargetActive}
          fileTree={{
            activeOutlineIndex: activeDocumentOutlineIndex,
            autoRevealActiveFile: editorPreferences.preferences.autoRevealActiveFile,
            currentPath: currentFileTreePath,
            customTemplates: markdownTemplates,
            documentLinksOpen,
            documentLinksVisible,
            fileListVisible: viewModeChrome.fileList,
            fileTreeAssetsVisible,
            fileTreeSort,
            files: fileTreeFiles,
            folderOpen: Boolean(fileTree.sourcePath),
            language: appLanguage.language,
            linkIndex: workspaceLinkIndex.index,
            linkIndexLoading: workspaceLinkIndex.loading,
            maxWidth: compactViewport
              ? Math.max(fileTreeMinWidth, viewportWidth - 48)
              : fileTreeMaxWidth,
            minWidth: compactViewport
              ? Math.min(fileTreeMinWidth, Math.max(0, viewportWidth - 48))
              : fileTreeMinWidth,
            open: visibleFileTreeOpen,
            outlineItems,
            outlineVisible: viewModeChrome.outline,
            revealPathRequest: fileTreeRevealPathRequest,
            resizing: fileTreeResizing,
            rootPath: fileTree.sourcePath,
            rootName: fileTreeRootName,
            sidebarLayoutMode,
            syncState: sidebarSyncState,
            updateAvailable: Boolean(appUpdater.availableUpdate),
            width: compactViewport
              ? Math.min(fileTreeWidth, Math.max(0, viewportWidth - 48))
              : fileTreeWidth,
            onCleanUnusedImages: assetCleanupAvailable ? assetCleanup.openDialog : undefined,
            onCreateFile: handleCreateMarkdownTreeFile,
            onCreateFolder: handleCreateMarkdownTreeFolder,
            onDeleteFile: handleDeleteMarkdownTreeFile,
            onDocumentLinksOpenChange: handleDocumentLinksOpenChange,
            onFileTreeAssetsVisibleChange: setFileTreeAssetsVisible,
            onFileTreeSortChange: setFileTreeSort,
            onInsertImageAsset: handleInsertFileTreeImageAsset,
            onMoveFile: handleMoveMarkdownTreeFile,
            onOpenContainingFolder: handleOpenContainingFolder,
            onOpenFile: handleOpenTreeFile,
            onOpenFileToSide: editorPreferences.preferences.showDocumentTabs
              ? handleOpenTreeFileToSide
              : undefined,
            onOpenFolder: canChooseLocalWorkspace ? handleOpenMarkdownFolder : undefined,
            onOpenSettings: handleOpenSettings,
            onSyncNow: sidebarSyncAvailable ? runApplicationSyncNow : undefined,
            onInstallAvailableUpdate: appUpdater.installAvailableUpdate,
            onRenameFile: handleRenameMarkdownTreeFile,
            onResize: compactViewport ? undefined : resizeFileTree,
            onResizeEnd: compactViewport ? undefined : endFileTreeResize,
            onResizeStart: compactViewport ? undefined : startFileTreeResize,
            onSaveFileAsTemplate: handleSaveMarkdownFileAsTemplate,
            onSelectOutlineItem: editor.selectOutlineItem,
            onToggleMarkdownFiles: handleFileTreeToggle
          }}
          windowsSelfDrawnChrome={windowsSelfDrawnChromeEnabled}
          workspaceLayoutClassName={workspaceLayoutClassName}
          workspaceLayoutStyle={visibleWorkspaceLayoutStyle}
          onEditorContentDragLeave={handleEditorContentDragLeave}
          onEditorContentDragOver={handleEditorContentDragOver}
          onEditorContentDrop={handleEditorContentDrop}
        >
              {documentSearchOverlay}
              {globalSearchOpen ? (
                <GlobalSearchPanel
                  caseSensitive={globalSearchCaseSensitive}
                  language={appLanguage.language}
                  loading={globalSearchLoading}
                  query={globalSearchQuery}
                  recentQueries={globalSearchRecentQueries}
                  results={globalSearchResponse.results}
                  searchedFileCount={globalSearchResponse.searchedFileCount}
                  truncated={globalSearchResponse.truncated}
                  unreadableFileCount={globalSearchResponse.unreadableFileCount}
                  onCaseSensitiveChange={handleGlobalSearchCaseSensitiveChange}
                  onClose={handleGlobalSearchClose}
                  onOpenResult={handleGlobalSearchResultOpen}
                  onQueryChange={handleGlobalSearchQueryChange}
                  onRecentQuerySelect={handleGlobalSearchRecentQuerySelect}
                />
              ) : null}
              {quickOpenOpen ? (
                <QuickOpenPanel
                  currentPath={currentFileTreePath}
                  files={fileTreeFiles}
                  language={appLanguage.language}
                  openFilePaths={quickOpenFilePaths}
                  onClose={handleQuickOpenClose}
                  onOpenFile={handleQuickOpenFileOpen}
                />
              ) : null}
              {activeImageFile ? (
                <ImagePreview
                  alt={activeImageFile.name}
                  language={appLanguage.language}
                  src={imagePreviewSrc}
                />
              ) : hasOpenDocument ? (
                <div
                  className={
                    sideDocumentOpen
                      ? "editor-side-by-side-surface grid h-full min-h-0 grid-cols-1 grid-rows-[minmax(0,1fr)_minmax(0,1fr)] divide-y divide-(--border-default) min-[960px]:grid-cols-[minmax(0,var(--side-document-main-pane))_8px_minmax(0,var(--side-document-secondary-pane))] min-[960px]:grid-rows-[minmax(0,1fr)] min-[960px]:divide-y-0"
                      : "relative h-full min-h-0"
                  }
                  ref={sideDocumentOpen ? sideDocumentSurfaceRef : undefined}
                  style={sideDocumentOpen ? sideDocumentSurfaceStyle : undefined}
                >
                  <div
                    className="relative h-full min-h-0 overflow-hidden"
                    ref={mainDocumentPaneRef}
                    onFocusCapture={handleMainDocumentPaneFocus}
                  >
                    {splitMode ? (
                    <div
                      className="editor-split-surface grid h-full min-h-0 grid-cols-1 grid-rows-[minmax(0,1fr)_minmax(0,1fr)] divide-y divide-(--border-default) min-[900px]:grid-cols-[minmax(0,var(--split-visual-pane))_8px_minmax(0,var(--split-source-pane))] min-[900px]:grid-rows-[minmax(0,1fr)] min-[900px]:divide-y-0"
                      ref={splitSurfaceRef}
                      style={splitSurfaceStyle}
                    >
                      <div className="min-h-0 overflow-hidden" onFocusCapture={handleVisualPaneFocus}>
                        {mainVisualEditors}
                      </div>
                      <div
                        className="group/split-resizer relative z-20 hidden cursor-col-resize touch-none outline-none min-[900px]:block"
                        role="separator"
                        tabIndex={0}
                        aria-label={translate("app.resizeSplitPanes")}
                        aria-orientation="vertical"
                        aria-valuemin={splitVisualPanePercentMin}
                        aria-valuemax={splitVisualPanePercentMax}
                        aria-valuenow={resolvedSplitVisualPanePercent}
                        onKeyDown={handleSplitPaneResizeKeyDown}
                        onPointerDown={handleSplitPaneResizePointerDown}
                      >
                        <span className="pointer-events-none absolute top-10 bottom-5 left-1/2 w-px -translate-x-1/2 bg-(--border-default) transition-colors duration-150 ease-out group-hover/split-resizer:bg-(--accent) group-focus/split-resizer:bg-(--accent)" />
                      </div>
                      <div className="min-h-0 overflow-hidden" onFocusCapture={handleSourcePaneFocus}>
                        <LazyMarkdownSourceEditor
                          key={activeTabId ?? "untitled:0"}
                          autoFocus={activeEditorSurface === "source"}
                          bottomOverlayInset={quietStatusOverlayInset}
                          bodyFontSize={editorPreferences.preferences.bodyFontSize}
                          content={document.content}
                          contentWidth={activeEditorContentWidth}
                          contentWidthPx={activeEditorContentWidthPx}
                          editorFontFamily={editorPreferences.preferences.editorFontFamily}
                          extendedSyntax={editorPreferences.preferences.extendedSyntax}
                          language={appLanguage.language}
                          lineHeight={editorPreferences.preferences.lineHeight}
                          onChange={(content) => handleSourceMarkdownTabChange(
                            activeTabId ?? "untitled:0",
                            content,
                            {
                              documentRevision: document.revision
                            }
                          )}
                          onContentWidthChange={editorWidthResizerVisible ? handleEditorContentWidthChange : undefined}
                          onContentWidthResizeEnd={editorWidthResizerVisible ? handleEditorContentWidthResizeEnd : undefined}
                          onScroll={handleSourcePaneScroll}
                          onSelectionTextChange={updateSelectedWordCount}
                          readOnly={mainEditorReadOnly}
                          searchActiveIndex={normalizedDocumentSearchActiveIndex}
                          searchMatches={visibleSourceDocumentSearchMatches}
                          showLineNumbers={editorPreferences.preferences.showLineNumbers}
                          scrollRef={sourceScrollRef}
                          topInset="titlebar"
                          typewriterModeEnabled={editorPreferences.preferences.typewriterModeEnabled}
                          vimModeEnabled={editorPreferences.preferences.vimModeEnabled}
                        />
                      </div>
                    </div>
                  ) : (
                    <div className="relative h-full min-h-0">
                      {mainVisualEditors}
                      {sourceMode ? (
                        <LazyMarkdownSourceEditor
                          key={activeTabId ?? "untitled:0"}
                          autoFocus
                          bottomOverlayInset={quietStatusOverlayInset}
                          bodyFontSize={editorPreferences.preferences.bodyFontSize}
                          content={document.content}
                          contentWidth={activeEditorContentWidth}
                          contentWidthPx={activeEditorContentWidthPx}
                          editorFontFamily={editorPreferences.preferences.editorFontFamily}
                          extendedSyntax={editorPreferences.preferences.extendedSyntax}
                          language={appLanguage.language}
                          lineHeight={editorPreferences.preferences.lineHeight}
                          onChange={(content) => handleSourceMarkdownTabChange(
                            activeTabId ?? "untitled:0",
                            content,
                            {
                              documentRevision: document.revision
                            }
                          )}
                          onContentWidthChange={editorWidthResizerVisible ? handleEditorContentWidthChange : undefined}
                          onContentWidthResizeEnd={editorWidthResizerVisible ? handleEditorContentWidthResizeEnd : undefined}
                          onScroll={handleSourcePaneScroll}
                          onSelectionTextChange={updateSelectedWordCount}
                          readOnly={mainEditorReadOnly}
                          searchActiveIndex={normalizedDocumentSearchActiveIndex}
                          searchMatches={visibleSourceDocumentSearchMatches}
                          showLineNumbers={editorPreferences.preferences.showLineNumbers}
                          scrollRef={sourceScrollRef}
                          topInset="titlebar"
                          typewriterModeEnabled={editorPreferences.preferences.typewriterModeEnabled}
                          vimModeEnabled={editorPreferences.preferences.vimModeEnabled}
                        />
                      ) : null}
                    </div>
                  )}
                  {viewModeChrome.statusBar ? (
                    <QuietStatus
                      dirty={document.dirty}
                      language={appLanguage.language}
                      readOnly={mainEditorReadOnly}
                      selectedWordCount={selectedWordCount}
                      showWordCount={viewModeChrome.wordCount && editorPreferences.preferences.showWordCount}
                      syncLabel={syncStatusLabel}
                      wordCount={wordCount}
                    />
                  ) : null}
                  </div>
                  {sideDocumentOpen && sideDocumentTab ? (
                    <>
                      <div
                        className="group/side-resizer relative z-20 hidden cursor-col-resize touch-none outline-none min-[960px]:block"
                        role="separator"
                        tabIndex={0}
                        aria-label={translate("app.resizeSideBySideDocuments")}
                        aria-orientation="vertical"
                        aria-valuemin={sideDocumentMainPanePercentMin}
                        aria-valuemax={sideDocumentMainPanePercentMax}
                        aria-valuenow={resolvedSideDocumentMainPanePercent}
                        onKeyDown={handleSideDocumentPaneResizeKeyDown}
                        onPointerDown={handleSideDocumentPaneResizePointerDown}
                      >
                        <span className="pointer-events-none absolute top-10 bottom-5 left-1/2 w-px -translate-x-1/2 bg-(--border-default) transition-colors duration-150 ease-out group-hover/side-resizer:bg-(--accent) group-focus/side-resizer:bg-(--accent)" />
                      </div>
                      <SideDocumentPane
                        bottomOverlayInset={viewModeChrome.statusBar ? quietStatusOverlayInset : 0}
                        bodyFontSize={editorPreferences.preferences.bodyFontSize}
                        content={sideDocumentTab.content}
                        contentWidth={activeEditorContentWidth}
                        contentWidthPx={activeEditorContentWidthPx}
                        documentKey={sideDocumentTab.id}
                        documentPath={sideDocumentTab.path}
                        editorFontFamily={editorPreferences.preferences.editorFontFamily}
                        editorTheme={appTheme.editorTheme}
                        extendedSyntax={editorPreferences.preferences.extendedSyntax}
                        language={appLanguage.language}
                        lineHeight={editorPreferences.preferences.lineHeight}
                        markdownShortcuts={editorPreferences.preferences.markdownShortcuts}
                        paragraphSpacingPx={editorPreferences.preferences.paragraphSpacingPx}
                        mode={sourceMode ? "source" : "visual"}
                        onSaveEditorResources={(request) => handleSaveEditorResources(request, sideDocumentTab.path)}
                        openLocalAttachment={(src) => handleOpenLocalAttachment(src, sideDocumentTab.path)}
                        openExternalUrl={handleOpenEditorLink}
                        readOnly={sideEditorReadOnly}
                        resolveImageSrc={resolveSideDocumentImageSrc}
                        revision={sideDocumentTab.revision}
                        sizeBytes={sideDocumentTab.sizeBytes}
                        hideHeadingMarkersOnFocus={editorPreferences.preferences.hideHeadingMarkersOnFocus}
                        showLineNumbers={editorPreferences.preferences.showLineNumbers}
                        status={viewModeChrome.statusBar ? (
                          <QuietStatus
                            dirty={sideDocumentTab.dirty}
                            language={appLanguage.language}
                            readOnly={sideEditorReadOnly}
                            showWordCount={viewModeChrome.wordCount && editorPreferences.preferences.showWordCount}
                            wordCount={sideDocumentWordCount}
                          />
                        ) : null}
                        tableColumnWidthMode={editorPreferences.preferences.tableColumnWidthMode}
                        workspaceFiles={fileTreeFiles}
                        onChange={handleSideDocumentChange}
                        onContentWidthChange={editorWidthResizerVisible ? handleEditorContentWidthChange : undefined}
                        onContentWidthResizeEnd={editorWidthResizerVisible ? handleEditorContentWidthResizeEnd : undefined}
                        onFocus={handleSideDocumentPaneFocus}
                        wrapCodeBlocks={editorPreferences.preferences.wrapCodeBlocks}
                      />
                    </>
                  ) : null}
                </div>
              ) : null}
        </WorkspaceLayout>

        <SelectionToolbar
          activeFormattingActions={selectionToolbarActiveActions}
          activeHeadingLevel={selectionToolbarHeadingLevel}
          anchor={selectionToolbarAnchor}
          copySucceeded={selectionToolbarCopySucceeded}
          language={appLanguage.language}
          onCopySelection={handleSelectionToolbarCopySelection}
          onDismiss={handleSelectionToolbarDismiss}
          onInsertLink={handleSelectionToolbarInsertLink}
          onRunFormattingAction={handleSelectionToolbarFormattingAction}
          onSetHeadingLevel={handleSelectionToolbarHeadingLevelAction}
          open={selectionToolbarAnchor !== null}
        />
        </main>
        {exportFeatureEnabled ? (
          <MarkdownExportDocument
            snapshot={exportSnapshot}
            extendedSyntax={editorPreferences.preferences.extendedSyntax}
            resolveImageSrc={resolveExportImageSrc}
            onRendered={handleRenderedExport}
          />
        ) : null}
        {desktopRemoteNotebookDialog}
      </div>
      {settingsModal}
    </>
  );
}
