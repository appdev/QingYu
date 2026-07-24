import { useEffect, useState } from "react";
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
import { useSettingsWindowState } from "../hooks/useSettingsWindowState";
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
import { getAppRuntime } from "../runtime";
import type { SettingsCategory } from "../hooks/useSettingsWindowState";
import { requestPrimaryNotebookSwitch } from "../lib/notebook-switch-events";

export function SettingsWindow() {
  const settingsState = useSettingsWindowState();
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
    primaryWorkspace,
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
  const showWindowsWindowChrome = platform === "windows" && appFeatures.nativeWindowChrome;
  const showMacosWindowChrome = platform === "macos" && appFeatures.nativeWindowChrome;
  const liveSettingsStartupReady = appLanguage.ready && appTheme.ready;
  const [settingsStartupReady, setSettingsStartupReady] = useState(liveSettingsStartupReady);
  useEffect(() => {
    if (!settingsStartupReady && liveSettingsStartupReady) setSettingsStartupReady(true);
  }, [liveSettingsStartupReady, settingsStartupReady]);
  const settingsLayoutClassName = showWindowsWindowChrome
    ? "settings-layout absolute inset-x-0 top-10 bottom-0 grid grid-cols-[180px_minmax(0,1fr)]"
    : "settings-layout grid h-screen grid-cols-[180px_minmax(0,1fr)]";
  const handleCloseSettings = () => {
    hideSettingsWindow().catch(() => {});
  };
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
    if (!settingsStartupReady) return;

    markSettingsWindowReady().catch(() => {});
  }, [settingsStartupReady]);

  if (!settingsStartupReady) return null;

  return (
    <main
      className="settings-window relative h-screen overflow-hidden overscroll-none bg-(--bg-primary) text-(--text-primary)"
      aria-label={translate("settings.aria.main")}
    >
      <AppToaster language={appLanguage.language} />
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
      {showWindowsWindowChrome ? (
        <header
          className="settings-window-chrome fixed inset-x-0 top-0 z-30 grid h-10 grid-cols-[minmax(0,1fr)_auto] select-none items-center bg-(--bg-chrome) [-webkit-user-select:none]"
          aria-label={translate("settings.aria.dragRegion")}
          data-tauri-drag-region
        >
          <div
            className="relative z-20 flex h-10 items-center px-3 text-[12px] leading-none font-[620] text-(--text-heading)"
            data-tauri-drag-region
          >
            QingYu
          </div>
          <div
            className="pointer-events-none absolute top-0 left-1/2 z-10 flex h-10 -translate-x-1/2 items-center justify-center px-6 text-[12px] leading-none font-[620] text-(--text-heading)"
            data-tauri-drag-region
          >
            {translate("settings.title")}
          </div>
          <WindowsWindowControls onClose={handleCloseSettings} />
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
          onClose={platform === "linux" ? handleCloseSettings : undefined}
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
              translate={translate}
              onDetectPandocPath={handleDetectPandocPath}
              onFocusTargetHandled={clearSettingsFocusTarget}
              onUpdateSettings={handleUpdateExportSettings}
            />
          ) : null}
        </SettingsContent>
      </div>
    </main>
  );
}
