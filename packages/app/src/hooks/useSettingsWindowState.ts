import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t, type I18nKey } from "@markra/shared";
import {
  getStoredEditorPreferences,
  getStoredExportSettings,
  getStoredFileIgnoreSettings,
  defaultEditorPreferences,
  defaultExportSettings,
  defaultFileIgnoreSettings,
  resetWelcomeDocumentState,
  exportStoredAppSettings,
  importStoredAppSettings,
  saveStoredEditorPreferences,
  saveStoredExportSettings,
  saveStoredFileIgnoreSettings,
  normalizeExportSettings,
  normalizeFileIgnoreSettings,
  type EditorPreferences,
  type ExportSettings,
  type FileIgnoreSettings,
  type PortableStoredAppSettings
} from "../lib/settings/app-settings";
import {
  listenAppEditorPreferencesChanged,
  listenAppFileIgnoreSettingsChanged,
  notifyAppEditorPreferencesChanged,
  notifyAppExportSettingsChanged,
  notifyAppFileIgnoreSettingsChanged,
  notifyAppLanguageChanged,
  notifyAppThemeChanged
} from "../lib/settings/settings-events";
import {
  detectNativePandocPath,
  deleteNativeMarkdownTemplateFile,
  getNativeShellCommandStatus,
  installNativeShellCommand,
  openNativeSettingsFile,
  readNativeMarkdownTemplateFile,
  saveNativeSettingsFile,
  uninstallNativeShellCommand,
  writeNativeMarkdownTemplateFile
} from "../lib/tauri";
import type { NativeShellCommandStatus } from "../lib/tauri/shell-command";
import { showAppToast } from "../lib/app-toast";
import {
  acknowledgeSettingsWindowHide,
  cancelSettingsWindowHide,
  completeSettingsWindowHide,
  hideSettingsWindow,
  listenNativeSettingsWindowTarget,
  listenNativeSettingsWindowHideRequested,
  type NativeSettingsWindowContext,
  type NativeSettingsWindowHideRequest,
  type NativeSettingsWindowTarget
} from "../lib/tauri/window";
import { requestPrimaryCloudNotebookCatalog } from "../lib/cloud-notebook-catalog-events";
import { normalizeSystemFontFamilyName } from "../lib/editor-font";
import {
  loadMarkdownTemplatesFromEntries,
  markdownTemplateEntryFromTemplate,
  type MarkdownTemplate
} from "../lib/templates";
import { useAppLanguage } from "./useAppLanguage";
import { useAppTheme } from "./useAppTheme";
import { getAppRuntime, type AppSystemFontFamily } from "../runtime";
import { useCompactSyncSettings } from "./useCompactSyncSettings";
import { usePrimaryWorkspace } from "./usePrimaryWorkspace";

export type SettingsCategory =
  | "general"
  | "notesWorkspace"
  | "sync"
  | "mcp"
  | "logs"
  | "resources"
  | "appearance"
  | "view"
  | "editor"
  | "templates"
  | "keyboardShortcuts"
  | "export";

export type SettingsFocusTarget = "pandocPath";

const settingsExportSuggestedName = "markra-settings.json";

function settingsTargetFromSearch(search: string): NativeSettingsWindowTarget | null {
  const target = new URLSearchParams(search).get("settingsTarget");

  return target === "exportPandocPath" || target === "sync" ? target : null;
}

function settingsCategoryForTarget(target: NativeSettingsWindowTarget | null): SettingsCategory {
  if (target === "exportPandocPath") return "export";
  if (target === "sync") return "sync";
  return "general";
}

function settingsFocusTargetForNativeTarget(target: NativeSettingsWindowTarget | null): SettingsFocusTarget | null {
  return target === "exportPandocPath" ? "pandocPath" : null;
}

function settingsWindowContextFromSearch(search: string) {
  const params = new URLSearchParams(search);
  const workspaceContextProvided = params.get("settingsWorkspaceContext") === "1";
  const sourceWindowLabel = params.get("settingsSourceWindowLabel")?.trim() || null;
  const workspaceSourcePath = params.get("settingsWorkspaceSourcePath")?.trim() || null;

  return {
    sourceWindowLabel: workspaceContextProvided ? sourceWindowLabel : null,
    workspaceContextProvided,
    workspaceSourcePath: workspaceContextProvided ? workspaceSourcePath : null
  };
}

export function shellCommandActionFailureMessage(baseMessage: string, error: unknown) {
  const detail = error instanceof Error
    ? error.message.trim()
    : typeof error === "string"
      ? error.trim()
      : "";

  return detail ? `${baseMessage} ${detail}` : baseMessage;
}

export function canonicalizeEditorFontFamilyPreference(
  preferences: EditorPreferences,
  systemFontFamilies: readonly AppSystemFontFamily[]
): EditorPreferences | null {
  if (preferences.editorFontFamily.source !== "system") return null;

  const savedFamily = normalizeSystemFontFamilyName(preferences.editorFontFamily.family);
  if (!savedFamily) return null;

  const matchesSavedFamily = systemFontFamilies.some(
    (fontFamily) => normalizeSystemFontFamilyName(fontFamily.family) === savedFamily
  );
  if (matchesSavedFamily) return null;

  const labelMatches = new Map<string, string>();
  for (const fontFamily of systemFontFamilies) {
    const family = normalizeSystemFontFamilyName(fontFamily.family);
    const label = normalizeSystemFontFamilyName(fontFamily.label);

    if (!family || label !== savedFamily) continue;
    labelMatches.set(family, family);
  }

  if (labelMatches.size !== 1) return null;

  const canonicalFamily = Array.from(labelMatches.keys())[0];
  if (!canonicalFamily) return null;

  return {
    ...preferences,
    editorFontFamily: {
      family: canonicalFamily,
      source: "system"
    }
  };
}

export function useSettingsWindowState() {
  const appTheme = useAppTheme();
  const appLanguage = useAppLanguage();
  const primaryWorkspace = usePrimaryWorkspace({ trueMobile: false });
  const initialSettingsTarget = settingsTargetFromSearch(window.location.search);
  const initialWindowContext = settingsWindowContextFromSearch(window.location.search);
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>(() =>
    settingsCategoryForTarget(initialSettingsTarget)
  );
  const nativeSettingsTargetRef = useRef<NativeSettingsWindowTarget | null>(null);
  const [settingsFocusTarget, setSettingsFocusTarget] = useState<SettingsFocusTarget | null>(() =>
    settingsFocusTargetForNativeTarget(initialSettingsTarget)
  );
  const [settingsWorkspaceContext, setSettingsWorkspaceContext] = useState(() => ({
    resolved: initialWindowContext.workspaceContextProvided,
    sourceWindowLabel: initialWindowContext.sourceWindowLabel,
    workspaceSourcePath: initialWindowContext.workspaceSourcePath
  }));
  const [syncPrimaryRoot, setSyncPrimaryRoot] = useState<string | null>(null);
  const [syncPrimaryRootResolved, setSyncPrimaryRootResolved] = useState(false);
  const [settingsTransferRunning, setSettingsTransferRunning] = useState(false);
  const [editorPreferences, setEditorPreferences] = useState<EditorPreferences>(defaultEditorPreferences);
  const [markdownTemplates, setMarkdownTemplates] = useState<MarkdownTemplate[]>([]);
  const [exportSettings, setExportSettings] = useState<ExportSettings>(defaultExportSettings);
  const [fileIgnoreSettings, setFileIgnoreSettings] = useState<FileIgnoreSettings>(defaultFileIgnoreSettings);
  const [shellCommandStatus, setShellCommandStatus] = useState<NativeShellCommandStatus | null>(null);
  const [shellCommandRunning, setShellCommandRunning] = useState(false);
  const [systemFontFamilies, setSystemFontFamilies] = useState<AppSystemFontFamily[]>([]);
  const [welcomeReset, setWelcomeReset] = useState(false);
  const syncSession = useCompactSyncSettings({
    available: true,
    observedLoadResult: null,
    primaryRoot: syncPrimaryRoot,
    shouldBegin: activeCategory === "sync" && syncPrimaryRootResolved
  });
  const syncView = useMemo(() => ({
    configDocument: syncSession.configDocument,
    loadResult: syncSession.loadResult,
    primaryRoot: syncPrimaryRoot,
    saving: syncSession.saving,
    status: syncSession.status,
    syncRunning: syncSession.syncRunning,
    testing: syncSession.testing
  }), [
    syncPrimaryRoot,
    syncSession.configDocument,
    syncSession.loadResult,
    syncSession.saving,
    syncSession.status,
    syncSession.syncRunning,
    syncSession.testing,
  ]);
  const syncPrimaryRootRef = useRef<string | null>(syncPrimaryRoot);
  syncPrimaryRootRef.current = syncPrimaryRoot;
  const syncPrimaryRootResolvedRef = useRef(syncPrimaryRootResolved);
  syncPrimaryRootResolvedRef.current = syncPrimaryRootResolved;
  const syncPrimaryContextGenerationRef = useRef(0);
  const settingsHideInFlightRef = useRef(new Map<number, Promise<unknown>>());
  const cloudNotebookCatalogRequestRef = useRef<Promise<unknown> | null>(null);
  const translate = useCallback((key: I18nKey) => t(appLanguage.language, key), [appLanguage.language]);
  const showSyncExitFailure = useCallback(() => {
    showAppToast({
      id: "application-sync-settings-exit",
      message: translate("settings.sync.failed"),
      status: "error"
    });
  }, [translate]);
  const handleSelectCategory = useCallback(async (category: SettingsCategory) => {
    if (activeCategory === "sync" && category !== "sync") {
      try {
        await syncSession.end("category-leave");
      } catch {
        showSyncExitFailure();
        return;
      }
    }
    setActiveCategory(category);
    setSettingsFocusTarget(null);
  }, [activeCategory, showSyncExitFailure, syncSession.end]);
  const recoverVisibleSyncSession = useCallback(async () => {
    try {
      await syncSession.begin();
    } catch {
      // The window stays visible with failure feedback even if session recovery is unavailable.
    }
  }, [syncSession.begin]);
  const handleSelectCloudNotebook = useCallback(() => {
    const pendingRequest = cloudNotebookCatalogRequestRef.current;
    if (pendingRequest) return pendingRequest;

    let request!: Promise<unknown>;
    request = (async () => {
      let editingEnded = false;
      try {
        await syncSession.end("catalog-handoff");
        editingEnded = true;
        await requestPrimaryCloudNotebookCatalog();
        await hideSettingsWindow();
      } catch {
        if (editingEnded) {
          await recoverVisibleSyncSession();
        }
        showSyncExitFailure();
        if (cloudNotebookCatalogRequestRef.current === request) {
          cloudNotebookCatalogRequestRef.current = null;
        }
      }
    })();
    cloudNotebookCatalogRequestRef.current = request;
    return request;
  }, [recoverVisibleSyncSession, showSyncExitFailure, syncSession.end]);
  const handleSettingsWindowTarget = useCallback((target: NativeSettingsWindowTarget) => {
    nativeSettingsTargetRef.current = target;
    handleSelectCategory(settingsCategoryForTarget(target)).catch(() => {});
    setSettingsFocusTarget(settingsFocusTargetForNativeTarget(target));
  }, [handleSelectCategory]);
  const clearSettingsFocusTarget = useCallback(() => {
    setSettingsFocusTarget(null);
  }, []);

  useLayoutEffect(() => {
    document.documentElement.dataset.window = "settings";

    return () => {
      delete document.documentElement.dataset.window;
    };
  }, []);

  useLayoutEffect(() => {
    document.title = translate("settings.title");
  }, [translate]);

  const applySyncPrimaryRoot = useCallback(async (
    nextPrimaryRoot: string | null,
    contextGeneration: number
  ) => {
    if (contextGeneration !== syncPrimaryContextGenerationRef.current) return;
    if (syncPrimaryRootRef.current !== nextPrimaryRoot) {
      syncPrimaryRootRef.current = nextPrimaryRoot;
      syncPrimaryRootResolvedRef.current = true;
      setSyncPrimaryRoot(nextPrimaryRoot);
      setSyncPrimaryRootResolved(true);
      return;
    }

    if (!syncPrimaryRootResolvedRef.current) {
      syncPrimaryRootResolvedRef.current = true;
      setSyncPrimaryRootResolved(true);
      return;
    }

    await syncSession.begin();
  }, [syncSession.begin]);

  const handleSettingsWindowContext = useCallback((context: NativeSettingsWindowContext) => {
    const reopenTarget = nativeSettingsTargetRef.current;
    nativeSettingsTargetRef.current = null;
    setSettingsWorkspaceContext({
      resolved: true,
      sourceWindowLabel: context.sourceWindowLabel,
      workspaceSourcePath: context.workspaceSourcePath
    });
    if (
      (reopenTarget !== null && reopenTarget !== "sync")
      || activeCategory !== "sync"
      || !syncPrimaryRootResolvedRef.current
    ) {
      cloudNotebookCatalogRequestRef.current = null;
      return Promise.resolve(undefined);
    }

    let reopening!: Promise<unknown>;
    reopening = recoverVisibleSyncSession().then(() => {
      if (cloudNotebookCatalogRequestRef.current === reopening) {
        cloudNotebookCatalogRequestRef.current = null;
      }
    });
    cloudNotebookCatalogRequestRef.current = reopening;
    return reopening;
  }, [activeCategory, recoverVisibleSyncSession]);

  useEffect(() => {
    if (primaryWorkspace.status === "loading") return;

    const contextGeneration = ++syncPrimaryContextGenerationRef.current;
    const nextPrimaryRoot = primaryWorkspace.status === "ready"
      ? primaryWorkspace.root
      : null;
    applySyncPrimaryRoot(nextPrimaryRoot, contextGeneration).catch(() => {
      showSyncExitFailure();
    });
  }, [
    applySyncPrimaryRoot,
    primaryWorkspace.root,
    primaryWorkspace.status,
    showSyncExitFailure
  ]);

  const completeRequestedSettingsHide = useCallback((request: NativeSettingsWindowHideRequest) => {
    const pendingHide = settingsHideInFlightRef.current.get(request.generation);
    if (pendingHide) return pendingHide;

    const acknowledgement = acknowledgeSettingsWindowHide(request.generation);
    const shutdown = syncSession.end("window-close");
    const hidePromise = Promise.allSettled([acknowledgement, shutdown])
      .then((results) => {
        const failed = results.find((result) => result.status === "rejected");
        if (failed?.status === "rejected") throw failed.reason;
      })
      .then(() => completeSettingsWindowHide(request.generation))
      .then(() => {
        cloudNotebookCatalogRequestRef.current = null;
      })
      .catch(async () => {
        try {
          await cancelSettingsWindowHide(request.generation);
        } catch {
          // The retryable frontend session remains active even if native cancellation fails.
        }
        if (cloudNotebookCatalogRequestRef.current) {
          await recoverVisibleSyncSession();
          cloudNotebookCatalogRequestRef.current = null;
        }
        showSyncExitFailure();
      })
      .finally(() => {
        if (settingsHideInFlightRef.current.get(request.generation) === hidePromise) {
          settingsHideInFlightRef.current.delete(request.generation);
        }
      });
    settingsHideInFlightRef.current.set(request.generation, hidePromise);
    return hidePromise;
  }, [recoverVisibleSyncSession, showSyncExitFailure, syncSession.end]);

  useEffect(() => {
    let cancelled = false;
    let stopListening: (() => unknown) | null = null;

    listenNativeSettingsWindowHideRequested((request) => completeRequestedSettingsHide(request)).then((cleanup) => {
      if (cancelled) {
        cleanup();
        return;
      }
      stopListening = cleanup;
    }).catch(() => {});

    return () => {
      cancelled = true;
      stopListening?.();
    };
  }, [completeRequestedSettingsHide]);

  useEffect(() => {
    let cancelled = false;
    let stopListening: (() => unknown) | null = null;

    listenNativeSettingsWindowTarget((target) => {
      if (!cancelled) handleSettingsWindowTarget(target);
    }, (context) => cancelled ? undefined : handleSettingsWindowContext(context)).then((cleanup) => {
      if (cancelled) {
        cleanup();
        return;
      }

      stopListening = cleanup;
    }).catch(() => {});

    return () => {
      cancelled = true;
      stopListening?.();
    };
  }, [handleSettingsWindowContext, handleSettingsWindowTarget]);

  useEffect(() => {
    let cancelled = false;
    let stopListening: (() => unknown) | null = null;

    getStoredFileIgnoreSettings().then((settings) => {
      if (!cancelled) setFileIgnoreSettings(settings);
    }).catch(() => {});

    listenAppFileIgnoreSettingsChanged((settings) => {
      if (!cancelled) setFileIgnoreSettings(settings);
    }).then((cleanup) => {
      if (cancelled) {
        cleanup();
        return;
      }

      stopListening = cleanup;
    }).catch(() => {});

    return () => {
      cancelled = true;
      stopListening?.();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    getAppRuntime().systemFonts.listFontFamilies()
      .then((fontFamilies) => {
        if (!cancelled) setSystemFontFamilies(fontFamilies);
      })
      .catch(() => {
        if (!cancelled) setSystemFontFamilies([]);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    getStoredExportSettings().then((settings) => {
      if (!cancelled) setExportSettings(settings);
    }).catch(() => {});

    return () => {
      cancelled = true;
    };
  }, []);

  const refreshShellCommandStatus = useCallback(() => {
    getNativeShellCommandStatus()
      .then((status) => setShellCommandStatus(status))
      .catch(() => setShellCommandStatus({ commandPath: null, targetPath: null, status: "unavailable" }));
  }, []);

  useEffect(() => {
    let cancelled = false;

    getNativeShellCommandStatus()
      .then((status) => {
        if (!cancelled) setShellCommandStatus(status);
      })
      .catch(() => {
        if (!cancelled) setShellCommandStatus({ commandPath: null, targetPath: null, status: "unavailable" });
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let stopListening: (() => unknown) | null = null;

    getStoredEditorPreferences().then((preferences) => {
      if (cancelled) return;

      setEditorPreferences(preferences);
      loadMarkdownTemplatesFromEntries(preferences.markdownTemplates, readNativeMarkdownTemplateFile)
        .then((templates) => {
          if (!cancelled) setMarkdownTemplates(templates);
        })
        .catch(() => {
          if (!cancelled) setMarkdownTemplates([]);
        });
    }).catch(() => {});

    listenAppEditorPreferencesChanged((preferences) => {
      if (cancelled) return;

      setEditorPreferences(preferences);
      loadMarkdownTemplatesFromEntries(preferences.markdownTemplates, readNativeMarkdownTemplateFile)
        .then((templates) => {
          if (!cancelled) setMarkdownTemplates(templates);
        })
        .catch(() => {
          if (!cancelled) setMarkdownTemplates([]);
        });
    }).then((cleanup) => {
      if (cancelled) {
        cleanup();
        return;
      }

      stopListening = cleanup;
    }).catch(() => {});

    return () => {
      cancelled = true;
      stopListening?.();
    };
  }, []);

  const handleResetWelcomeDocument = useCallback(() => {
    resetWelcomeDocumentState().then(() => {
      setWelcomeReset(true);
    }).catch(() => {});
  }, []);

  const handleUpdateEditorPreferences = useCallback((preferences: EditorPreferences) => {
    setEditorPreferences(preferences);
    saveStoredEditorPreferences(preferences)
      .then(() => notifyAppEditorPreferencesChanged(preferences))
      .catch(() => {});
  }, []);

  const handleApplyFileIgnoreSettings = useCallback((settings: FileIgnoreSettings) => {
    const normalizedSettings = normalizeFileIgnoreSettings(settings);
    saveStoredFileIgnoreSettings(normalizedSettings)
      .then(() => {
        setFileIgnoreSettings(normalizedSettings);
        return notifyAppFileIgnoreSettingsChanged(normalizedSettings);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    const nextPreferences = canonicalizeEditorFontFamilyPreference(editorPreferences, systemFontFamilies);
    if (!nextPreferences) return;

    handleUpdateEditorPreferences(nextPreferences);
  }, [editorPreferences, handleUpdateEditorPreferences, systemFontFamilies]);

  const handleSaveMarkdownTemplate = useCallback((template: MarkdownTemplate) => {
    const entry = markdownTemplateEntryFromTemplate(template);
    const nextPreferences = {
      ...editorPreferences,
      markdownTemplates: editorPreferences.markdownTemplates.some((storedTemplate) => storedTemplate.id === entry.id)
        ? editorPreferences.markdownTemplates.map((storedTemplate) => storedTemplate.id === entry.id ? entry : storedTemplate)
        : [...editorPreferences.markdownTemplates, entry]
    };
    const nextTemplate = {
      ...template,
      fileName: entry.fileName
    };

    setEditorPreferences(nextPreferences);
    setMarkdownTemplates((currentTemplates) =>
      currentTemplates.some((storedTemplate) => storedTemplate.id === nextTemplate.id)
        ? currentTemplates.map((storedTemplate) => storedTemplate.id === nextTemplate.id ? nextTemplate : storedTemplate)
        : [...currentTemplates, nextTemplate]
    );

    writeNativeMarkdownTemplateFile(entry.fileName, template.content)
      .then(() => saveStoredEditorPreferences(nextPreferences))
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences]);

  const handleDeleteMarkdownTemplate = useCallback((template: MarkdownTemplate) => {
    const entry = editorPreferences.markdownTemplates.find((storedTemplate) => storedTemplate.id === template.id);
    const nextPreferences = {
      ...editorPreferences,
      markdownTemplates: editorPreferences.markdownTemplates.filter((storedTemplate) => storedTemplate.id !== template.id)
    };

    setEditorPreferences(nextPreferences);
    setMarkdownTemplates((currentTemplates) =>
      currentTemplates.filter((storedTemplate) => storedTemplate.id !== template.id)
    );

    const deleteTemplateFile = entry
      ? deleteNativeMarkdownTemplateFile(entry.fileName)
      : Promise.resolve();

    deleteTemplateFile
      .then(() => saveStoredEditorPreferences(nextPreferences))
      .then(() => notifyAppEditorPreferencesChanged(nextPreferences))
      .catch(() => {});
  }, [editorPreferences]);

  const handleUpdateExportSettings = useCallback((settings: ExportSettings) => {
    const normalizedSettings = normalizeExportSettings(settings);
    setExportSettings(normalizedSettings);
    saveStoredExportSettings(normalizedSettings)
      .then(() => notifyAppExportSettingsChanged(normalizedSettings))
      .catch(() => {});
  }, []);
  const handleDetectPandocPath = useCallback(() => {
    detectNativePandocPath().then((path) => {
      if (!path) {
        showAppToast({
          message: translate("settings.export.pandocPathNotFound"),
          status: "error"
        });
        return;
      }

      handleUpdateExportSettings({
        ...exportSettings,
        pandocPath: path
      });
      showAppToast({
        message: translate("settings.export.pandocPathDetected"),
        status: "success"
      });
    }).catch(() => {
      showAppToast({
        message: translate("settings.export.pandocPathNotFound"),
        status: "error"
      });
    });
  }, [exportSettings, handleUpdateExportSettings, translate]);

  const handleInstallShellCommand = useCallback(() => {
    if (shellCommandRunning) return;

    setShellCommandRunning(true);
    installNativeShellCommand()
      .then((status) => {
        setShellCommandStatus(status);
        showAppToast({
          message: translate("settings.shellCommand.installSucceeded"),
          status: "success"
        });
      })
      .catch((error) => {
        refreshShellCommandStatus();
        showAppToast({
          message: shellCommandActionFailureMessage(translate("settings.shellCommand.actionFailed"), error),
          status: "error"
        });
      })
      .finally(() => setShellCommandRunning(false));
  }, [refreshShellCommandStatus, shellCommandRunning, translate]);

  const handleUninstallShellCommand = useCallback(() => {
    if (shellCommandRunning) return;

    setShellCommandRunning(true);
    uninstallNativeShellCommand()
      .then((status) => {
        setShellCommandStatus(status);
        showAppToast({
          message: translate("settings.shellCommand.uninstallSucceeded"),
          status: "success"
        });
      })
      .catch((error) => {
        refreshShellCommandStatus();
        showAppToast({
          message: shellCommandActionFailureMessage(translate("settings.shellCommand.actionFailed"), error),
          status: "error"
        });
      })
      .finally(() => setShellCommandRunning(false));
  }, [refreshShellCommandStatus, shellCommandRunning, translate]);

  const applyImportedSettings = useCallback((settings: PortableStoredAppSettings) => {
    setEditorPreferences(settings.editorPreferences);
    setExportSettings(settings.exportSettings);
    setFileIgnoreSettings(settings.fileIgnoreSettings);
    loadMarkdownTemplatesFromEntries(settings.editorPreferences.markdownTemplates, readNativeMarkdownTemplateFile)
      .then((templates) => setMarkdownTemplates(templates))
      .catch(() => setMarkdownTemplates([]));

    notifyAppEditorPreferencesChanged(settings.editorPreferences).catch(() => {});
    notifyAppExportSettingsChanged(settings.exportSettings).catch(() => {});
    notifyAppFileIgnoreSettingsChanged(settings.fileIgnoreSettings).catch(() => {});
    notifyAppLanguageChanged(settings.language).catch(() => {});
    notifyAppThemeChanged({
      appearanceMode: settings.appearanceMode,
      darkTheme: settings.darkTheme,
      lightTheme: settings.lightTheme
    }).catch(() => {});
  }, []);

  const handleExportSettings = useCallback(async () => {
    if (settingsTransferRunning) return;

    setSettingsTransferRunning(true);
    try {
      const contents = await exportStoredAppSettings();
      const savedFile = await saveNativeSettingsFile({
        contents,
        suggestedName: settingsExportSuggestedName
      });
      if (savedFile) {
        showAppToast({
          message: translate("settings.storage.exportSucceeded"),
          status: "success"
        });
      }
    } catch {
      showAppToast({
        message: translate("settings.storage.exportFailed"),
        status: "error"
      });
    } finally {
      setSettingsTransferRunning(false);
    }
  }, [settingsTransferRunning, translate]);

  const handleImportSettings = useCallback(async () => {
    if (settingsTransferRunning) return;

    setSettingsTransferRunning(true);
    try {
      const file = await openNativeSettingsFile({
        title: translate("settings.storage.importPickerTitle")
      });
      if (!file) return;

      const settings = await importStoredAppSettings(file.content);
      applyImportedSettings(settings);
      showAppToast({
        message: translate("settings.storage.importSucceeded"),
        status: "success"
      });
    } catch {
      showAppToast({
        message: translate("settings.storage.importFailed"),
        status: "error"
      });
    } finally {
      setSettingsTransferRunning(false);
    }
  }, [applyImportedSettings, settingsTransferRunning, translate]);

  return {
    activeCategory,
    appLanguage,
    appTheme,
    editorPreferences,
    exportSettings,
    fileIgnoreSettings,
    handleExportSettings,
    handleApplyFileIgnoreSettings,
    handleImportSettings,
    handleResetWelcomeDocument,
    handleCreateMarkdownTemplate: handleSaveMarkdownTemplate,
    handleDeleteMarkdownTemplate,
    handleUpdateMarkdownTemplate: handleSaveMarkdownTemplate,
    handleInstallShellCommand,
    handleSelectCloudNotebook,
    handleUpdateEditorPreferences,
    handleUpdateExportSettings,
    handleDetectPandocPath,
    handleRefreshShellCommandStatus: refreshShellCommandStatus,
    handleUninstallShellCommand,
    setActiveCategory: handleSelectCategory,
    markdownTemplates,
    primaryWorkspace,
    settingsFocusTarget,
    settingsSourceWindowLabel: settingsWorkspaceContext.sourceWindowLabel,
    settingsTransferRunning,
    settingsWorkspaceContextResolved: settingsWorkspaceContext.resolved,
    settingsWorkspaceSourcePath: settingsWorkspaceContext.workspaceSourcePath,
    shellCommandRunning,
    shellCommandStatus,
    syncView,
    syncSession,
    systemFontFamilies,
    clearSettingsFocusTarget,
    translate,
    welcomeReset
  };
}
