import { diagnosticErrorMessage } from "@markra/shared";
import { Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { Tooltip } from "@markra/ui";
import {
  appAppearanceModeOptions,
  forgetApprovedThemeFingerprint,
  type AppAppearanceMode
} from "../../lib/settings/app-settings";
import type { ThemeDescriptor } from "../../lib/themes/theme-catalog";
import type { useAppTheme } from "../../hooks/useAppTheme";
import {
  SettingsRow,
  SettingsSection
} from "./SettingsControls";
import {
  ThemeCatalogSection,
  ThemeCatalogToolbar
} from "./ThemeSettingsControls";
import { mergeClassNames } from "./class-names";
import type { SettingsTranslate } from "./translate";

const appearanceModeIcons: Record<AppAppearanceMode, LucideIcon> = {
  dark: Moon,
  light: Sun,
  system: Monitor
};

type AppearanceThemeController = Pick<ReturnType<typeof useAppTheme>,
  | "activeTheme"
  | "appearanceMode"
  | "catalog"
  | "darkTheme"
  | "lightTheme"
  | "selectAppearanceMode"
  | "selectTheme"
  | "themeError"
>;

function AppearanceModeControl({
  onSelectAppearanceMode,
  selectedAppearanceMode,
  translate
}: {
  onSelectAppearanceMode: (mode: AppAppearanceMode) => unknown;
  selectedAppearanceMode: AppAppearanceMode;
  translate: SettingsTranslate;
}) {
  return (
    <div
      className="inline-flex overflow-hidden rounded-md border border-(--border-default) bg-(--bg-primary)"
      role="radiogroup"
      aria-label={translate("settings.theme.appearanceModeTitle")}
    >
      {appAppearanceModeOptions.map((mode) => {
        const Icon = appearanceModeIcons[mode];
        const selected = mode === selectedAppearanceMode;
        const tooltipLabel = translate(mode === "system"
          ? "settings.theme.useSystemLabel"
          : mode === "dark"
            ? "settings.theme.useDarkLabel"
            : "settings.theme.useLightLabel");

        return (
          <Tooltip key={mode} content={tooltipLabel}>
            <button
              className={mergeClassNames(
                "inline-flex h-8 min-w-20 cursor-pointer items-center justify-center gap-1.5 border-0 border-r border-(--border-default) bg-transparent px-3 text-[12px] leading-5 font-[620] text-(--text-secondary) transition-colors duration-150 ease-out last:border-r-0 hover:bg-(--bg-hover) hover:text-(--text-heading) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-inset",
                selected ? "bg-(--bg-active) text-(--text-heading)" : ""
              )}
              type="button"
              role="radio"
              aria-checked={selected}
              onClick={() => onSelectAppearanceMode(mode)}
            >
              <Icon aria-hidden="true" size={13} />
              {translate(mode === "system"
                ? "settings.theme.system"
                : mode === "dark"
                  ? "settings.theme.dark"
                  : "settings.theme.light")}
            </button>
          </Tooltip>
        );
      })}
    </div>
  );
}

export function AppearanceSettings({
  themeController,
  translate
}: {
  themeController: AppearanceThemeController;
  translate: SettingsTranslate;
}) {
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { catalog } = themeController;

  useEffect(() => {
    catalog.refresh().catch(() => {});
  }, [catalog.refresh]);

  async function runAction(action: () => Promise<unknown>) {
    setBusy(true);
    setActionError(null);
    try {
      await action();
    } catch (error) {
      setActionError(diagnosticErrorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  function handleImport() {
    runAction(async () => {
      const result = await catalog.importTheme();
      if (result?.kind !== "conflict") return;
      if (!window.confirm(`${translate("settings.theme.replaceTheme")} “${result.existing.name}”?`)) return;
      await catalog.replaceTheme(result.sourcePath, result.existing.fingerprint);
    }).catch(() => {});
  }

  function handleDelete(theme: ThemeDescriptor) {
    if (!window.confirm(`${translate("settings.theme.deleteTheme")} “${theme.name}”?`)) return;
    runAction(async () => {
      await catalog.deleteTheme(theme);
      await forgetApprovedThemeFingerprint(theme.id);
    }).catch(() => {});
  }

  const diagnostics = [actionError, themeController.themeError, catalog.error].filter((message): message is string => Boolean(message));

  return (
    <SettingsSection label={translate("settings.sections.theme")}>
      <SettingsRow
        title={translate("settings.theme.appearanceModeTitle")}
        description={translate("settings.theme.description")}
        action={
          <AppearanceModeControl
            selectedAppearanceMode={themeController.appearanceMode}
            translate={translate}
            onSelectAppearanceMode={themeController.selectAppearanceMode}
          />
        }
      />
      <ThemeCatalogToolbar
        capabilities={catalog.capabilities}
        disabled={busy}
        translate={translate}
        onImport={handleImport}
        onOpenDirectory={() => runAction(() => catalog.openDirectory()).catch(() => {})}
        onRefresh={() => runAction(() => catalog.refresh()).catch(() => {})}
      />
      <ThemeCatalogSection
        canDelete={catalog.capabilities.canDelete}
        selectedThemeId={themeController.lightTheme}
        themes={catalog.lightThemes}
        title={translate("settings.theme.lightPaletteTitle")}
        translate={translate}
        onDelete={handleDelete}
        onSelect={themeController.selectTheme}
      />
      <ThemeCatalogSection
        canDelete={catalog.capabilities.canDelete}
        selectedThemeId={themeController.darkTheme}
        themes={catalog.darkThemes}
        title={translate("settings.theme.darkPaletteTitle")}
        translate={translate}
        onDelete={handleDelete}
        onSelect={themeController.selectTheme}
      />
      {diagnostics.length > 0 ? (
        <div className="rounded-md border border-(--border-default) bg-(--bg-secondary) px-3 py-2 text-[11px] leading-4.5 text-(--text-secondary)" role="alert">
          <p className="m-0 font-[650] text-(--text-heading)">{translate("settings.theme.actionFailed")}</p>
          {diagnostics.map((message) => <p key={message} className="m-0 mt-1">{message}</p>)}
        </div>
      ) : null}
      {catalog.invalidFiles.length > 0 ? (
        <details className="rounded-md border border-(--border-default) bg-(--bg-secondary) px-3 py-2 text-[11px] leading-4.5 text-(--text-secondary)">
          <summary className="cursor-pointer font-[650] text-(--text-heading)">
            {translate("settings.theme.invalidFiles")} ({catalog.invalidFiles.length})
          </summary>
          <ul className="mb-0 pl-4">
            {catalog.invalidFiles.map((file) => <li key={file.fileName}>{file.fileName}: {file.reason}</li>)}
          </ul>
        </details>
      ) : null}
    </SettingsSection>
  );
}
