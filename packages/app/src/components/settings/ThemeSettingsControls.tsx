import {
  ChevronDown,
  ChevronUp,
  FolderOpen,
  RefreshCw,
  Trash2,
  Upload
} from "lucide-react";
import { useMemo, useState, type CSSProperties } from "react";
import type {
  ThemeDescriptor,
  ThemeRuntimeCapabilities
} from "../../lib/themes/theme-catalog";
import { mergeClassNames } from "./class-names";
import { SettingsButton } from "./SettingsControls";
import type { SettingsTranslate } from "./translate";

const collapsedThemeLimit = 12;

type ThemePreviewStyle = CSSProperties & {
  "--theme-preview-accent": string;
  "--theme-preview-bg": string;
  "--theme-preview-panel": string;
  "--theme-preview-text": string;
};

function previewStyle(theme: ThemeDescriptor): ThemePreviewStyle {
  return {
    "--theme-preview-accent": theme.preview.accent,
    "--theme-preview-bg": theme.preview.background,
    "--theme-preview-panel": theme.preview.panel,
    "--theme-preview-text": theme.preview.text
  };
}

export function ThemeCatalogToolbar({
  capabilities,
  disabled,
  onImport,
  onOpenDirectory,
  onRefresh,
  translate
}: {
  capabilities: ThemeRuntimeCapabilities;
  disabled?: boolean;
  onImport: () => unknown;
  onOpenDirectory: () => unknown;
  onRefresh: () => unknown;
  translate: SettingsTranslate;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2 py-3" aria-label={translate("settings.sections.theme")}>
      {capabilities.canImport ? (
        <SettingsButton
          disabled={disabled}
          label={translate("settings.theme.importTheme")}
          onClick={onImport}
        >
          <Upload aria-hidden="true" size={13} />
          {translate("settings.theme.importTheme")}
        </SettingsButton>
      ) : null}
      <SettingsButton
        disabled={disabled}
        label={translate("settings.theme.refreshThemes")}
        onClick={onRefresh}
      >
        <RefreshCw aria-hidden="true" size={13} />
        {translate("settings.theme.refreshThemes")}
      </SettingsButton>
      {capabilities.canOpenDirectory ? (
        <SettingsButton
          disabled={disabled}
          label={translate("settings.theme.openThemeFolder")}
          onClick={onOpenDirectory}
        >
          <FolderOpen aria-hidden="true" size={13} />
          {translate("settings.theme.openThemeFolder")}
        </SettingsButton>
      ) : null}
    </div>
  );
}

function ThemeCard({
  canDelete,
  onDelete,
  onSelect,
  selected,
  theme,
  translate
}: {
  canDelete: boolean;
  onDelete: (theme: ThemeDescriptor) => unknown;
  onSelect: (theme: ThemeDescriptor) => unknown;
  selected: boolean;
  theme: ThemeDescriptor;
  translate: SettingsTranslate;
}) {
  return (
    <div
      className={mergeClassNames(
        "group relative min-w-0 rounded-lg border bg-(--bg-primary) p-2 transition-colors",
        selected ? "border-(--accent) ring-1 ring-(--accent)" : "border-(--border-default) hover:bg-(--bg-hover)"
      )}
    >
      <button
        className="block w-full cursor-pointer border-0 bg-transparent p-0 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
        type="button"
        role="radio"
        aria-checked={selected}
        aria-label={theme.name}
        onClick={() => onSelect(theme)}
      >
        <span
          className="relative mb-2 block h-13 overflow-hidden rounded-md border border-black/10"
          style={previewStyle(theme)}
          aria-hidden="true"
        >
          <span className="absolute inset-0" style={{ background: "var(--theme-preview-bg)" }} />
          <span className="absolute top-2 right-2 bottom-2 left-2 rounded" style={{ background: "var(--theme-preview-panel)" }} />
          <span className="absolute top-4 left-4 h-1.5 w-1/2 rounded-full" style={{ background: "var(--theme-preview-text)" }} />
          <span className="absolute top-7 left-4 h-1 w-1/3 rounded-full opacity-60" style={{ background: "var(--theme-preview-text)" }} />
          <span className="absolute right-4 bottom-3 size-3 rounded-full" style={{ background: "var(--theme-preview-accent)" }} />
        </span>
        <span className="block truncate text-[12px] leading-4.5 font-[650] text-(--text-heading)">{theme.name}</span>
        <span className="mt-0.5 block truncate text-[10px] leading-4 text-(--text-secondary)">
          {theme.source === "default" ? translate("settings.theme.defaultBadge") : theme.author ?? translate("settings.theme.thirdPartyBadge")}
        </span>
      </button>
      {canDelete && theme.source === "third-party" ? (
        <button
          className="absolute top-3 right-3 flex size-6 cursor-pointer items-center justify-center rounded-md border border-(--border-default) bg-(--bg-primary)/90 text-(--text-secondary) opacity-0 shadow-sm transition-opacity group-hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
          type="button"
          aria-label={`${translate("settings.theme.deleteTheme")}: ${theme.name}`}
          onClick={() => onDelete(theme)}
        >
          <Trash2 aria-hidden="true" size={12} />
        </button>
      ) : null}
    </div>
  );
}

export function ThemeCatalogSection({
  canDelete,
  onDelete,
  onSelect,
  selectedThemeId,
  themes,
  title,
  translate
}: {
  canDelete: boolean;
  onDelete: (theme: ThemeDescriptor) => unknown;
  onSelect: (theme: ThemeDescriptor) => unknown;
  selectedThemeId: string;
  themes: ThemeDescriptor[];
  title: string;
  translate: SettingsTranslate;
}) {
  const [expanded, setExpanded] = useState(false);
  const visibleThemes = useMemo(() => {
    if (expanded || themes.length <= collapsedThemeLimit) return themes;
    const selected = themes.find(({ id }) => id === selectedThemeId);
    if (!selected || themes.indexOf(selected) < collapsedThemeLimit) return themes.slice(0, collapsedThemeLimit);

    return [themes[0], selected, ...themes.slice(1).filter(({ id }) => id !== selected.id)]
      .slice(0, collapsedThemeLimit)
      .filter((theme): theme is ThemeDescriptor => Boolean(theme));
  }, [expanded, selectedThemeId, themes]);

  return (
    <div className="py-4">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h4 className="m-0 text-[13px] leading-5 font-[650] text-(--text-heading)">{title}</h4>
        <span className="text-[11px] text-(--text-secondary)">{themes.length}</span>
      </div>
      <div
        className="grid grid-cols-4 gap-2 max-[980px]:grid-cols-3 max-[720px]:grid-cols-2"
        role="radiogroup"
        aria-label={title}
      >
        {visibleThemes.map((theme) => (
          <ThemeCard
            key={`${theme.id}:${theme.fingerprint}`}
            canDelete={canDelete}
            selected={theme.id === selectedThemeId}
            theme={theme}
            translate={translate}
            onDelete={onDelete}
            onSelect={onSelect}
          />
        ))}
      </div>
      {themes.length > collapsedThemeLimit ? (
        <button
          className="mt-3 inline-flex cursor-pointer items-center gap-1 border-0 bg-transparent p-0 text-[11px] font-[600] text-(--accent) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
          type="button"
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? <ChevronUp aria-hidden="true" size={13} /> : <ChevronDown aria-hidden="true" size={13} />}
          {translate(expanded ? "settings.theme.showLess" : "settings.theme.showMore")}
        </button>
      ) : null}
    </div>
  );
}
