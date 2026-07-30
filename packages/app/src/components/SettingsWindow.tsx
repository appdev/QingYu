import { useCallback, useEffect, useRef, useState } from "react";
import { AppToaster } from "./AppToaster";
import {
  AppearanceSettings,
  EditorSettings,
  ExportSettings,
  GeneralSettings,
  KeyboardShortcutsSettings,
  McpSettings,
  NotesWorkspaceSettings,
  ResourcesSettings,
  RuntimeLogSettings,
  SyncSettings,
  TemplatesSettings,
  ViewSettings
} from "./SettingsSections";
import { SettingsContent, SettingsSidebar } from "./SettingsShell";
import {
  useSettingsWindowState,
  type SettingsWindowPresentation
} from "../hooks/useSettingsWindowState";
import { useAutoUpdater } from "../hooks/useAutoUpdater";
import { useDefaultContextMenuBlocker } from "../hooks/useDefaultContextMenuBlocker";
import { useRuntimeLogCapture } from "../hooks/useRuntimeLogCapture";
import { useRuntimeLogEntries } from "../hooks/useRuntimeLogEntries";
import { appLogger } from "../lib/app-logger";
import { appVersion } from "../lib/app-version";
import { showAppToast } from "../lib/app-toast";
import { resolveDesktopPlatform } from "../lib/platform";
import { hideSettingsWindow, markSettingsWindowReady } from "../lib/tauri";
import { MacWindowControls } from "./MacWindowControls";
import { WindowsWindowControls } from "./WindowsWindowControls";
import {
  getAppRuntime,
  type NativeSettingsWindowContext,
  type NativeSettingsWindowTarget
} from "../runtime";
import type { SettingsCategory } from "../hooks/useSettingsWindowState";
import { requestPrimaryNotebookSwitch } from "../lib/notebook-switch-events";
import { RemoteNotebookDialog } from "./notebooks/RemoteNotebookDialog";
import { SyncConflictHistoryDialog } from "./sync/SyncConflictHistoryDialog";
import type { SyncConflictRecord } from "../lib/sync-config";
import { SettingsWindowLoadingShell } from "./SettingsWindowLoadingShell";
import { SettingsModalFrame } from "./SettingsModalFrame";

type OpenSyncConflictHistory = {
  conflict: SyncConflictRecord;
  notesRoot: string;
};

type SettingsWindowProps = {
  context?: NativeSettingsWindowContext;
  initialTarget?: NativeSettingsWindowTarget;
  onClose?: () => unknown | Promise<unknown>;
  presentation?: SettingsWindowPresentation;
};

export function SettingsWindow({
  context,
  initialTarget,
  onClose,
  presentation = "window"
}: SettingsWindowProps = {}) {
  const settingsState = useSettingsWindowState({ context, initialTarget, presentation });
  const runtimeLog = useRuntimeLogEntries();
  const {
    activeCategory,
    appLanguage,
    appTheme,
    editorPreferences,
    exportSettings,
    fileIgnoreSettings,
    handleCreateMarkdownTemplate,
    handleDeleteMarkdownTemplate,
    handleResetWelcomeDocument,
    handleExportSettings,
    handleApplyFileIgnoreSettings,
    handleImportSettings,
    handleInstallShellCommand,
    handleSelectCloudNotebook,
    handleDetectPandocPath,
    handleRefreshShellCommandStatus,
    handleUninstallShellCommand,
    handleUpdateEditorPreferences,
    handleUpdateMarkdownTemplate,
    handleUpdateExportSettings,
    markdownTemplates,
    prepareSettingsClose,
    primaryWorkspace,
    remoteNotebookDialog,
    setActiveCategory,
    settingsFocusTarget,
    settingsSourceWindowLabel,
    settingsTransferRunning,
    settingsWorkspaceSourcePath,
    shellCommandRunning,
    shellCommandStatus,
    syncView,
    syncSession,
    systemFontFamilies,
    clearSettingsFocusTarget,
    translate,
    welcomeReset
  } = settingsState;
  const appRuntime = getAppRuntime();
  const appFeatures = appRuntime.features;
  const appLogs = appRuntime.logs;
  useRuntimeLogCapture();
  const hiddenCategories: SettingsCategory[] = [
    ...(appFeatures.export ? [] : (["export"] as SettingsCategory[])),
    ...(appFeatures.resources ? [] : (["resources"] as SettingsCategory[])),
    ...(appRuntime.mcp.policyAvailable ? [] : (["mcp"] as SettingsCategory[]))
  ];
  const activeSettingsCategory = hiddenCategories.includes(activeCategory) ? "general" : activeCategory;
  const platform = resolveDesktopPlatform();
  const modalPresentation = presentation === "modal";
  const windowsChromeLayout = platform === "windows" && appFeatures.nativeWindowChrome;
  const showWindowsWindowChrome = !modalPresentation && windowsChromeLayout;
  const showMacosWindowChrome = !modalPresentation && platform === "macos" && appFeatures.nativeWindowChrome;
  const liveSettingsStartupReady = appLanguage.ready && appTheme.ready;
  const [settingsStartupReady, setSettingsStartupReady] = useState(liveSettingsStartupReady);
  const [openSyncConflictHistory, setOpenSyncConflictHistory] = useState<OpenSyncConflictHistory | null>(null);
  const closePromiseRef = useRef<Promise<unknown> | null>(null);
  useEffect(() => {
    if (!settingsStartupReady && liveSettingsStartupReady) setSettingsStartupReady(true);
  }, [liveSettingsStartupReady, settingsStartupReady]);
  const settingsLayoutClassName = windowsChromeLayout
    ? "settings-layout absolute inset-x-0 top-10 bottom-0 grid grid-cols-[180px_minmax(0,1fr)] max-[700px]:grid-cols-1 max-[700px]:grid-rows-[auto_minmax(0,1fr)]"
    : `settings-layout grid ${modalPresentation ? "h-full" : "h-screen"} grid-cols-[180px_minmax(0,1fr)] max-[700px]:grid-cols-1 max-[700px]:grid-rows-[auto_minmax(0,1fr)]`;
  const handleCloseSettings = () => {
    setOpenSyncConflictHistory(null);
    if (!modalPresentation) {
      hideSettingsWindow().catch(() => {});
      return;
    }
    if (closePromiseRef.current) return closePromiseRef.current;

    const closePromise = prepareSettingsClose()
      .then((canClose) => canClose ? onClose?.() : undefined)
      .finally(() => {
        if (closePromiseRef.current === closePromise) closePromiseRef.current = null;
      });
    closePromiseRef.current = closePromise;
    return closePromise;
  };
  const handleOpenSyncConflictHistory = useCallback((conflict: SyncConflictRecord) => {
    if (!syncView.primaryRoot) return;
    setOpenSyncConflictHistory({ conflict, notesRoot: syncView.primaryRoot });
  }, [syncView.primaryRoot]);
  const handleSyncRepositoryIdentityChange = useCallback((identity: {
    notesRoot: string | null;
    repositoryId: string | null;
  }) => {
    setOpenSyncConflictHistory((current) => (
      current && (
        current.notesRoot !== identity.notesRoot
        || current.conflict.repositoryId !== identity.repositoryId
      )
        ? null
        : current
    ));
  }, []);
  useEffect(() => {
    setOpenSyncConflictHistory((current) => (
      current && current.notesRoot !== syncView.primaryRoot ? null : current
    ));
  }, [syncView.primaryRoot]);
  const handleReadSyncConflictHistory = useCallback(async (conflict: SyncConflictRecord) => {
    const selection = openSyncConflictHistory;
    if (
      !selection
      || selection.conflict.conflictId !== conflict.conflictId
      || selection.notesRoot !== syncView.primaryRoot
    ) {
      throw new Error("sync-conflict-ownership-changed");
    }
    const current = await getAppRuntime().syncConfig.loadRepositoryStatus({
      notesRoot: selection.notesRoot
    });
    if (current?.repositoryId !== conflict.repositoryId) {
      throw new Error("sync-conflict-ownership-changed");
    }
    return getAppRuntime().syncConfig.readDejavuConflictHistory({
      conflictId: conflict.conflictId,
      notesRoot: selection.notesRoot,
      repositoryId: conflict.repositoryId
    });
  }, [openSyncConflictHistory, syncView.primaryRoot]);
  const handleCopyRuntimeLogs = (contents: string) => {
    const writeText = navigator.clipboard?.writeText?.bind(navigator.clipboard);
    if (!writeText) {
      showAppToast({
        id: "runtime-log-copy",
        message: translate("settings.logs.copyFailed"),
        status: "error"
      });
      return;
    }

    writeText(contents).then(() => {
      showAppToast({
        id: "runtime-log-copy",
        message: translate("settings.logs.copySucceeded"),
        status: "success"
      });
    }).catch(() => {
      showAppToast({
        id: "runtime-log-copy",
        message: translate("settings.logs.copyFailed"),
        status: "error"
      });
    });
  };
  const handleOpenRuntimeLogFolder = appLogs.isAvailable()
    ? () => {
        appLogs.openLogFolder().catch((error) => {
          appLogger.warn("settings", "Open runtime log folder failed", { error });
          showAppToast({
            id: "runtime-log-open-folder",
            message: translate("settings.logs.openFolderFailed"),
            status: "error"
          });
        });
      }
    : undefined;
  useDefaultContextMenuBlocker();
  const updater = useAutoUpdater(appLanguage.language, appFeatures.updater && appLanguage.ready, {
    autoCheck: false,
    currentVersion: appVersion
  });
  useEffect(() => {
    if (!settingsStartupReady || modalPresentation) return;

    markSettingsWindowReady().catch(() => {});
  }, [modalPresentation, settingsStartupReady]);

  if (!settingsStartupReady) {
    return (
      <SettingsWindowLoadingShell
        onClose={handleCloseSettings}
        presentation={presentation}
      />
    );
  }

  const settingsContent = (
    <main
      className={`settings-window relative ${modalPresentation ? "h-full" : "h-screen"} overflow-hidden overscroll-none bg-(--bg-primary) text-(--text-primary)`}
      aria-label={translate("settings.aria.main")}
    >
      {!modalPresentation ? <AppToaster language={appLanguage.language} /> : null}
      {showMacosWindowChrome ? (
        <div
          className="settings-drag-region fixed inset-x-0 top-0 z-10 h-9.5 select-none [-webkit-user-select:none]"
          aria-label={translate("settings.aria.dragRegion")}
          data-tauri-drag-region
        />
      ) : null}
      {showMacosWindowChrome ? (
        <MacWindowControls
          className="fixed top-0 left-0 z-20 h-9.5"
          onClose={handleCloseSettings}
        />
      ) : null}
      {windowsChromeLayout ? (
        <header
          className={`settings-window-chrome ${modalPresentation ? "absolute" : "fixed"} inset-x-0 top-0 z-30 grid h-10 grid-cols-[minmax(0,1fr)_auto] select-none items-center bg-(--bg-chrome) [-webkit-user-select:none]`}
          aria-label={translate("settings.aria.dragRegion")}
          data-tauri-drag-region={showWindowsWindowChrome ? true : undefined}
        >
          <div
            className="relative z-20 flex h-10 items-center px-3 text-[12px] leading-none font-[620] text-(--text-heading)"
            data-tauri-drag-region={showWindowsWindowChrome ? true : undefined}
          >
            QingYu
          </div>
          <div
            className="pointer-events-none absolute top-0 left-1/2 z-10 flex h-10 -translate-x-1/2 items-center justify-center px-6 text-[12px] leading-none font-[620] text-(--text-heading)"
            data-tauri-drag-region={showWindowsWindowChrome ? true : undefined}
          >
            {translate("settings.title")}
          </div>
          {showWindowsWindowChrome ? <WindowsWindowControls onClose={handleCloseSettings} /> : null}
        </header>
      ) : null}
      <div className={settingsLayoutClassName}>
        <SettingsSidebar
          activeCategory={activeSettingsCategory}
          appVersion={appVersion}
          hiddenCategories={hiddenCategories}
          platform={platform}
          translate={translate}
          onCategoryChange={setActiveCategory}
        />
        <SettingsContent
          activeCategory={activeSettingsCategory}
          platform={platform}
          translate={translate}
          windowDragRegion={!modalPresentation}
          onClose={!modalPresentation && platform === "linux" ? handleCloseSettings : undefined}
        >
          {activeSettingsCategory === "general" ? (
            <GeneralSettings
              appVersion={appVersion}
              availableUpdateVersion={updater.availableUpdateVersion}
              fileIgnoreSettings={fileIgnoreSettings}
              preferences={editorPreferences}
              language={appLanguage.language}
              translate={translate}
              updatesEnabled={appFeatures.updater}
              welcomeReset={welcomeReset}
              onCheckForUpdates={updater.checkForUpdates}
              onApplyFileIgnoreSettings={handleApplyFileIgnoreSettings}
              onExportSettings={handleExportSettings}
              onImportSettings={handleImportSettings}
              onInstallShellCommand={handleInstallShellCommand}
              onRefreshShellCommand={handleRefreshShellCommandStatus}
              onResetWelcomeDocument={handleResetWelcomeDocument}
              onSelectLanguage={appLanguage.selectLanguage}
              onUninstallShellCommand={handleUninstallShellCommand}
              onUpdatePreferences={handleUpdateEditorPreferences}
              settingsTransferRunning={settingsTransferRunning}
              shellCommandRunning={shellCommandRunning}
              shellCommandStatus={shellCommandStatus}
            />
          ) : null}
          {activeSettingsCategory === "notesWorkspace" ? (
            <NotesWorkspaceSettings
              canChooseLocalRoot={primaryWorkspace.canChooseDesktopRoot}
              root={primaryWorkspace.root}
              status={primaryWorkspace.status}
              translate={translate}
              onChoose={() => requestPrimaryNotebookSwitch({ source: "settings" })}
              onResetOnboarding={primaryWorkspace.resetOnboarding}
            />
          ) : null}
          {activeSettingsCategory === "sync" ? (
            <SyncSettings
              configDocument={syncView.configDocument}
              loadResult={syncView.loadResult}
              primaryRoot={syncView.primaryRoot}
              saving={syncView.saving}
              status={syncView.status}
              syncRunning={syncView.syncRunning}
              testing={syncView.testing}
              translate={translate}
              onEnable={syncSession.enable}
              onOpenConflictHistory={handleOpenSyncConflictHistory}
              onRepositoryIdentityChange={handleSyncRepositoryIdentityChange}
              onPatch={syncSession.patch}
              onReset={syncSession.reset}
              onRunSync={syncSession.runImmediate}
              onSelectCloudNotebook={handleSelectCloudNotebook}
              onTestConnection={syncSession.testConnection}
            />
          ) : null}
          {activeSettingsCategory === "mcp" ? (
            <McpSettings translate={translate} />
          ) : null}
          {activeSettingsCategory === "logs" ? (
            <RuntimeLogSettings
              entries={runtimeLog.entries}
              translate={translate}
              onClearLogs={runtimeLog.clearEntries}
              onCopyLogs={handleCopyRuntimeLogs}
              onOpenLogFolder={handleOpenRuntimeLogFolder}
            />
          ) : null}
          {activeSettingsCategory === "resources" ? (
            <ResourcesSettings
              active
              globalIgnoreRules={fileIgnoreSettings.rules}
              sourceWindowLabel={settingsSourceWindowLabel}
              translate={translate}
              workspaceSourcePath={settingsWorkspaceSourcePath}
            />
          ) : null}
          {activeSettingsCategory === "appearance" ? (
            <AppearanceSettings
              themeController={appTheme}
              translate={translate}
            />
          ) : null}
          {activeSettingsCategory === "view" ? (
            <ViewSettings
              preferences={editorPreferences}
              translate={translate}
              onUpdatePreferences={handleUpdateEditorPreferences}
            />
          ) : null}
          {activeSettingsCategory === "editor" ? (
            <EditorSettings
              preferences={editorPreferences}
              systemFontFamilies={systemFontFamilies}
              translate={translate}
              onUpdatePreferences={handleUpdateEditorPreferences}
            />
          ) : null}
          {activeSettingsCategory === "templates" ? (
            <TemplatesSettings
              preferences={editorPreferences}
              templates={markdownTemplates}
              translate={translate}
              onCreateTemplate={handleCreateMarkdownTemplate}
              onDeleteTemplate={handleDeleteMarkdownTemplate}
              onUpdateTemplate={handleUpdateMarkdownTemplate}
            />
          ) : null}
          {activeSettingsCategory === "keyboardShortcuts" ? (
            <KeyboardShortcutsSettings
              newDocumentShortcutAvailable={appFeatures.nativeWindowChrome}
              platform={platform}
              preferences={editorPreferences}
              translate={translate}
              onUpdatePreferences={handleUpdateEditorPreferences}
            />
          ) : null}
          {appFeatures.export && activeSettingsCategory === "export" ? (
            <ExportSettings
              focusTarget={settingsFocusTarget}
              pandocEnabled={appFeatures.pandoc}
              settings={exportSettings}
              systemFontFamilies={systemFontFamilies}
              translate={translate}
              onDetectPandocPath={handleDetectPandocPath}
              onFocusTargetHandled={clearSettingsFocusTarget}
              onUpdateSettings={handleUpdateExportSettings}
            />
          ) : null}
        </SettingsContent>
      </div>
      {remoteNotebookDialog.open ? (
        <RemoteNotebookDialog
          allowCurrentNotebookSelection
          currentNotebookName={remoteNotebookDialog.currentNotebookName}
          entries={remoteNotebookDialog.entries}
          error={remoteNotebookDialog.error}
          language={appLanguage.language}
          loading={remoteNotebookDialog.loading}
          onCancel={remoteNotebookDialog.cancel}
          onRefresh={remoteNotebookDialog.refresh}
          onRestore={remoteNotebookDialog.restore}
        />
      ) : null}
      {openSyncConflictHistory ? (
        <SyncConflictHistoryDialog
          key={`${openSyncConflictHistory.notesRoot}\u0000${openSyncConflictHistory.conflict.repositoryId}\u0000${openSyncConflictHistory.conflict.conflictId}`}
          conflict={openSyncConflictHistory.conflict}
          language={appLanguage.language}
          onClose={() => setOpenSyncConflictHistory(null)}
          onRead={handleReadSyncConflictHistory}
        />
      ) : null}
    </main>
  );

  if (!modalPresentation) return settingsContent;

  return (
    <SettingsModalFrame
      closeLabel={translate("menu.closeWindow")}
      label={translate("settings.title")}
      onClose={handleCloseSettings}
      platform={platform}
    >
      {settingsContent}
    </SettingsModalFrame>
  );
}
