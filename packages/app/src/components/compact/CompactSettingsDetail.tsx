import { ArrowLeft, FolderOpen, Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { clampNumber, supportedLanguages, t, type I18nKey } from "@markra/shared";
import { useEffect } from "react";
import type {
  CompactNavigation,
  CompactSettingsCategory
} from "../../hooks/useCompactNavigation";
import {
  editorParagraphSpacingPxMax,
  editorParagraphSpacingPxMin,
  forgetApprovedThemeFingerprint,
  type AppAppearanceMode,
  type EditorPreferences
} from "../../lib/settings/app-settings";
import type { ThemeDescriptor } from "../../lib/themes/theme-catalog";
import { ThemeCatalogSection, ThemeCatalogToolbar } from "../settings/ThemeSettingsControls";
import type { CompactAppController } from "./types";
import { McpSettings } from "../settings/McpSettings";

type CompactSettingsDetailProps = {
  category: CompactSettingsCategory;
  controller: CompactAppController;
  navigation: CompactNavigation;
};

const controlClass = "min-h-11 min-w-11 w-full rounded-xl border border-(--border-subtle) bg-(--bg-secondary) px-3 text-sm text-(--text-heading) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)";
const bodyFontSizeOptions = [14, 15, 16, 17, 18, 20] as const;
const lineHeightOptions = [1.5, 1.65, 1.8] as const;
const appearanceOptions: Array<{
  icon: LucideIcon;
  labelKey: I18nKey;
  mode: AppAppearanceMode;
}> = [
  { icon: Monitor, labelKey: "settings.theme.system", mode: "system" },
  { icon: Sun, labelKey: "settings.theme.light", mode: "light" },
  { icon: Moon, labelKey: "settings.theme.dark", mode: "dark" }
];

const categoryLabelKeys: Record<CompactSettingsCategory, I18nKey> = {
  appearance: "compact.settings.appearance",
  editor: "compact.settings.editor",
  general: "compact.settings.general",
  mcp: "compact.settings.mcp",
  storage: "compact.settings.storage",
  sync: "compact.settings.sync"
};

function CompactSettingRow({
  children,
  description,
  title
}: {
  children: React.ReactNode;
  description?: string;
  title: string;
}) {
  return (
    <div className="grid min-w-0 gap-2 border-b border-(--border-subtle) py-4 last:border-b-0">
      <div className="min-w-0">
        <p className="m-0 text-sm font-semibold text-(--text-heading)">{title}</p>
        {description ? (
          <p className="m-0 mt-1 break-words text-xs text-(--text-secondary)">{description}</p>
        ) : null}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function CompactSwitch({
  checked,
  label,
  onChange
}: {
  checked: boolean;
  label: string;
  onChange: () => unknown;
}) {
  return (
    <button
      aria-checked={checked}
      aria-label={label}
      className={`${controlClass} flex items-center justify-between gap-3 text-left`}
      role="switch"
      type="button"
      onClick={onChange}
    >
      <span>{label}</span>
      <span
        aria-hidden="true"
        className={`relative h-6 w-11 shrink-0 rounded-full transition-colors ${checked ? "bg-(--accent)" : "bg-(--border-default)"}`}
      >
        <span className={`absolute top-1 size-4 rounded-full bg-white transition-transform ${checked ? "translate-x-6" : "translate-x-1"}`} />
      </span>
    </button>
  );
}

function formatStorageBytes(sizeBytes: number, language: CompactAppController["language"]) {
  const normalizedSize = Math.max(0, sizeBytes);
  if (normalizedSize < 1024) return `${normalizedSize} B`;
  return `${new Intl.NumberFormat(language, { maximumFractionDigits: 1 }).format(normalizedSize / 1024)} KB`;
}

function GeneralDetail({ controller }: { controller: CompactAppController }) {
  const language = controller.language;
  return (
    <CompactSettingRow
      description={t(language, "settings.language.description")}
      title={t(language, "settings.language.title")}
    >
      <select
        aria-label={t(language, "settings.language.title")}
        className={controlClass}
        value={language}
        onChange={(event) => controller.selectLanguage(event.currentTarget.value as CompactAppController["language"])}
      >
        {supportedLanguages.map((option) => (
          <option key={option.code} value={option.code}>{option.label}</option>
        ))}
      </select>
    </CompactSettingRow>
  );
}

function StorageDetail({ controller }: { controller: CompactAppController }) {
  const language = controller.language;
  const rootPath = controller.workspace.primaryRoot ?? controller.files.sourcePath ?? "—";
  const localFiles = controller.files.files.filter((file) => file.kind !== "folder");
  const totalSizeBytes = localFiles.reduce((total, file) => total + (file.sizeBytes ?? 0), 0);
  const syncMode = controller.workspace.syncConfigDocument?.readiness === "ready"
    && controller.workspace.syncConfigDocument.config.enabled;

  const values = [
    {
      label: controller.capabilities.trueMobile
        ? t(language, "compact.settings.managedWorkspace")
        : t(language, "compact.settings.currentProjectPath"),
      value: rootPath
    },
    { label: t(language, "compact.settings.localFiles"), value: String(localFiles.length) },
    { label: t(language, "compact.settings.totalSize"), value: formatStorageBytes(totalSizeBytes, language) },
    {
      label: t(language, "compact.settings.workspaceMode"),
      value: syncMode
        ? t(language, "compact.settings.syncWorkspace")
        : t(language, "compact.settings.localOnly")
    }
  ];

  return (
    <>
      <dl className="m-0 grid min-w-0 divide-y divide-(--border-subtle)">
        {values.map((item) => (
          <div className="grid min-w-0 gap-1 py-4" key={item.label}>
            <dt className="text-xs font-semibold text-(--text-secondary)">{item.label}</dt>
            <dd className="m-0 min-w-0 break-all text-sm text-(--text-heading)">{item.value}</dd>
          </div>
        ))}
      </dl>
      {controller.workspace.openNotebookManager ? (
        <button
          className={`${controlClass} mt-4 inline-flex items-center justify-center gap-2 whitespace-nowrap transition-colors duration-150 hover:bg-(--bg-hover) active:translate-y-px motion-reduce:transform-none motion-reduce:transition-none`}
          type="button"
          onClick={controller.workspace.openNotebookManager}
        >
          <FolderOpen aria-hidden="true" size={16} strokeWidth={1.7} />
          {controller.capabilities.trueMobile
            ? t(language, "compact.settings.manageNotebooks")
            : t(language, "compact.settings.switchNotebookDirectory")}
        </button>
      ) : null}
    </>
  );
}

function AppearanceDetail({ controller }: { controller: CompactAppController }) {
  const language = controller.language;
  const translate = (key: I18nKey) => t(language, key);
  const catalog = controller.appearance.catalog;

  useEffect(() => {
    catalog.refresh().catch(() => {});
  }, [catalog.refresh]);

  function deleteTheme(theme: ThemeDescriptor) {
    if (!window.confirm(`${translate("settings.theme.deleteTheme")} “${theme.name}”?`)) return;
    catalog.deleteTheme(theme)
      .then(() => forgetApprovedThemeFingerprint(theme.id))
      .catch(() => {});
  }

  return (
    <>
      <CompactSettingRow
        description={t(language, "settings.theme.description")}
        title={t(language, "settings.theme.appearanceModeTitle")}
      >
        <div className="grid min-w-0 grid-cols-3 gap-2" role="radiogroup" aria-label={t(language, "settings.theme.appearanceModeTitle")}>
          {appearanceOptions.map((option) => {
            const Icon = option.icon;
            return (
              <button
                aria-checked={controller.appearance.appearanceMode === option.mode}
                className={`${controlClass} inline-flex items-center justify-center gap-1.5 px-2 ${
                  controller.appearance.appearanceMode === option.mode ? "bg-(--bg-active)" : ""
                }`}
                key={option.mode}
                role="radio"
                type="button"
                onClick={() => controller.appearance.selectAppearanceMode(option.mode)}
              >
                <Icon aria-hidden="true" size={16} />
                <span className="truncate">{t(language, option.labelKey)}</span>
              </button>
            );
          })}
        </div>
      </CompactSettingRow>
      <ThemeCatalogToolbar
        capabilities={catalog.capabilities}
        translate={translate}
        onImport={() => catalog.importTheme()}
        onOpenDirectory={() => catalog.openDirectory()}
        onRefresh={() => catalog.refresh()}
      />
      <ThemeCatalogSection
        canDelete={catalog.capabilities.canDelete}
        selectedThemeId={controller.appearance.lightTheme}
        themes={catalog.lightThemes}
        title={translate("settings.theme.lightPaletteTitle")}
        translate={translate}
        onDelete={deleteTheme}
        onSelect={controller.appearance.selectTheme}
      />
      <ThemeCatalogSection
        canDelete={catalog.capabilities.canDelete}
        selectedThemeId={controller.appearance.darkTheme}
        themes={catalog.darkThemes}
        title={translate("settings.theme.darkPaletteTitle")}
        translate={translate}
        onDelete={deleteTheme}
        onSelect={controller.appearance.selectTheme}
      />
    </>
  );
}

function EditorDetail({ controller }: { controller: CompactAppController }) {
  const language = controller.language;
  const preferences = controller.preferences.preferences;
  const update = (patch: Partial<EditorPreferences>) => controller.preferences.updatePreferences({
    ...preferences,
    ...patch
  });

  return (
    <>
      {controller.capabilities.systemFonts ? (
        <CompactSettingRow
          description={t(language, "settings.editor.fontFamilyDescription")}
          title={t(language, "settings.editor.fontFamily")}
        >
          <input
            aria-label={t(language, "settings.editor.fontFamily")}
            autoCapitalize="none"
            autoCorrect="off"
            className={controlClass}
            placeholder={t(language, "settings.editor.fontFamily.theme")}
            spellCheck={false}
            type="text"
            value={preferences.editorFontFamily.family ?? ""}
            onChange={(event) => update({
              editorFontFamily: event.currentTarget.value.trim()
                ? { family: event.currentTarget.value, source: "system" }
                : { family: null, source: "theme" }
            })}
          />
        </CompactSettingRow>
      ) : null}
      <CompactSettingRow
        description={t(language, "settings.editor.bodyFontSizeDescription")}
        title={t(language, "settings.editor.bodyFontSize")}
      >
        <select
          aria-label={t(language, "settings.editor.bodyFontSize")}
          className={controlClass}
          value={preferences.bodyFontSize}
          onChange={(event) => update({ bodyFontSize: Number(event.currentTarget.value) })}
        >
          {bodyFontSizeOptions.map((size) => <option key={size} value={size}>{size}px</option>)}
        </select>
      </CompactSettingRow>
      <CompactSettingRow
        description={t(language, "settings.editor.lineHeightDescription")}
        title={t(language, "settings.editor.lineHeight")}
      >
        <select
          aria-label={t(language, "settings.editor.lineHeight")}
          className={controlClass}
          value={preferences.lineHeight}
          onChange={(event) => update({ lineHeight: Number(event.currentTarget.value) })}
        >
          {lineHeightOptions.map((height) => <option key={height} value={height}>{height}</option>)}
        </select>
      </CompactSettingRow>
      <CompactSettingRow
        description={t(language, "settings.editor.paragraphSpacingDescription")}
        title={t(language, "settings.editor.paragraphSpacing")}
      >
        <input
          aria-label={t(language, "settings.editor.paragraphSpacing")}
          className={controlClass}
          inputMode="numeric"
          max={editorParagraphSpacingPxMax}
          min={editorParagraphSpacingPxMin}
          type="number"
          value={preferences.paragraphSpacingPx}
          onChange={(event) => update({
            paragraphSpacingPx: clampNumber(
              Number(event.currentTarget.value),
              editorParagraphSpacingPxMin,
              editorParagraphSpacingPxMax
            ) ?? preferences.paragraphSpacingPx
          })}
        />
      </CompactSettingRow>
      <CompactSettingRow
        description={t(language, "settings.editor.wrapCodeBlocksDescription")}
        title={t(language, "settings.editor.wrapCodeBlocks")}
      >
        <CompactSwitch
          checked={preferences.wrapCodeBlocks}
          label={t(language, "settings.editor.wrapCodeBlocks")}
          onChange={() => update({ wrapCodeBlocks: !preferences.wrapCodeBlocks })}
        />
      </CompactSettingRow>
    </>
  );
}

function detailContent(category: CompactSettingsCategory, controller: CompactAppController) {
  if (category === "general") return <GeneralDetail controller={controller} />;
  if (category === "mcp") return (
    <McpSettings
      compact
      runtime={controller.mcp}
      translate={(key) => t(controller.language, key)}
    />
  );
  if (category === "storage") return <StorageDetail controller={controller} />;
  if (category === "appearance") return <AppearanceDetail controller={controller} />;
  if (category === "editor") return <EditorDetail controller={controller} />;
  return null;
}

export function CompactSettingsDetail({
  category,
  controller,
  navigation
}: CompactSettingsDetailProps) {
  const language = controller.language ?? "en";
  const title = t(language, categoryLabelKeys[category]);
  const back = () => navigation.pop().catch(() => {});

  return (
    <section
      aria-label={title}
      className="absolute inset-0 flex h-full min-h-0 min-w-0 flex-col overflow-x-hidden bg-(--bg-primary)"
      data-testid="compact-settings-detail"
    >
      <header className="flex shrink-0 items-center gap-2 border-b border-(--border-subtle) px-2 pt-[var(--compact-safe-area-top)]">
        <button
          aria-label={t(language, "compact.navigation.back")}
          className="inline-flex min-h-11 min-w-11 items-center justify-center rounded-lg"
          type="button"
          onClick={back}
        >
          <ArrowLeft aria-hidden="true" size={20} />
        </button>
        <h1 className="m-0 min-w-0 flex-1 truncate text-base font-semibold">{title}</h1>
      </header>
      <div
        className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden px-4 pb-[calc(1.25rem+var(--compact-bottom-inset))]"
        data-compact-scroll="vertical"
        data-compact-settings-scroll
      >
        <div className="mx-auto w-full min-w-0 max-w-lg">
          {detailContent(category, controller)}
        </div>
      </div>
    </section>
  );
}
