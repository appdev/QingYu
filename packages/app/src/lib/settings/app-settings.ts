import { defaultMarkdownShortcuts, normalizeMarkdownShortcuts, type MarkdownShortcutBindings } from "@markra/editor";
import { clampNumber, isAppLanguage, type AppLanguage } from "@markra/shared";
import {
  editorContentWidthOptions,
  normalizeEditorContentWidthPx,
  type EditorContentWidth
} from "../editor-width";
import {
  defaultEditorFontFamily,
  normalizeEditorFontFamilyPreference,
  type EditorFontFamilyPreference
} from "../editor-font";
import { normalizeMarkdownTemplateEntries, type MarkdownTemplateEntry } from "../templates";
import {
  defaultViewModeCustomizations,
  isViewMode,
  normalizeViewModeCustomizations,
  type ViewMode,
  type ViewModeCustomizations
} from "../view-mode";
import { getAppRuntime } from "../../runtime";
import {
  defaultExportSettings,
  normalizeExportSettings,
  normalizePortableExportSettings,
  type ExportSettings,
  type PdfMarginPreset,
  type PdfPageSize,
  type PortableExportSettings
} from "./export-settings";
import {
  defaultFileIgnoreSettings,
  normalizeFileIgnoreSettings,
  type FileIgnoreSettings
} from "./file-ignore-settings";
import { loadLocalPandocPath, saveLocalPandocPath } from "./local-state";

export {
  defaultExportSettings,
  normalizeExportSettings,
  normalizePortableExportSettings
} from "./export-settings";
export {
  clearStoredRecentMarkdownFiles,
  consumeWelcomeDocumentState,
  getStoredFileTreeSortByWorkspace,
  getStoredRecentMarkdownFiles,
  getStoredWorkspaceState,
  removeStoredRecentMarkdownFile,
  resetWelcomeDocumentState,
  saveStoredFileTreeSortForWorkspace,
  saveStoredRecentMarkdownFile,
  saveStoredWorkspaceState
} from "./local-state";
export type { PrimaryWorkspaceState } from "./local-state";
export {
  defaultFileIgnoreSettings,
  fileIgnoreRulesMaxLength,
  normalizeFileIgnoreRules,
  normalizeFileIgnoreSettings
} from "./file-ignore-settings";
export {
  normalizeRecentMarkdownFiles,
  prependRecentMarkdownFile
} from "./recent-markdown";
export {
  defaultStoredFileTreeSort,
  defaultWorkspaceState,
  normalizeFileTreeSortByWorkspace,
  normalizeStoredFileTreeSort,
  normalizeWorkspaceState
} from "./workspace-state";
export type {
  ExportSettings,
  PdfMarginPreset,
  PdfPageSize,
  PortableExportSettings
} from "./export-settings";
export type {
  FileIgnoreSettings
} from "./file-ignore-settings";
export type { McpConfig, McpSettingsSnapshot } from "../mcp";
export type {
  RecentMarkdownFile,
  RecentMarkdownFolder
} from "./recent-markdown";
export type {
  StoredFileTreeSort,
  StoredFileTreeSortByWorkspace,
  StoredWorkspaceDraftTab,
  StoredWorkspaceSideBySideGroup,
  StoredWorkspaceState,
  StoredWorkspaceWindowState,
  StoredWorkspaceWindow
} from "./workspace-state";

const settingsStorePath = "settings.json";
const themeKey = "theme";
const appearanceModeKey = "appearanceMode";
const lightThemeKey = "lightThemeId";
const darkThemeKey = "darkThemeId";
const legacyLightThemeKey = "lightTheme";
const legacyDarkThemeKey = "darkTheme";
const approvedThemeFingerprintsKey = "approvedThemeFingerprints";
const customThemeCssKey = "customThemeCss";
const lightCustomThemeCssKey = "lightCustomThemeCss";
const darkCustomThemeCssKey = "darkCustomThemeCss";
const languageKey = "language";
const editorPreferencesKey = "editorPreferences";
const fileIgnoreSettingsKey = "fileIgnoreSettings";
const exportSettingsKey = "exportSettings";
const storedAppSettingsFileFormat = "markra-settings";
const storedAppSettingsFileVersion = 3;
const invalidStoredAppSettingsFileMessage = "Invalid QingYu settings file.";

export type ResolvedAppTheme = "light" | "dark";
export const appAppearanceModeOptions = ["system", "light", "dark"] as const;
export type AppAppearanceMode = typeof appAppearanceModeOptions[number];
export type SidebarLayoutMode = "stacked" | "tabs";
export type TableColumnWidthModePreference = "auto" | "even";
export const editorThemeOptions = [
  "light",
  "dark",
  "github",
  "github-dark",
  "one-dark",
  "one-light",
  "one-dark-pro",
  "gothic",
  "newsprint",
  "night",
  "pixyll",
  "whitey",
  "sepia",
  "solarized-light",
  "solarized-dark",
  "nord",
  "catppuccin-latte",
  "catppuccin-mocha",
  "academic",
  "minimal",
  "custom"
] as const;
// Theme ids are discovered at runtime. The tuple above remains the legacy
// built-in list used only while importing older settings.
export type EditorTheme = string;
export const appThemeOptions = ["system", ...editorThemeOptions] as const;
export type AppTheme = typeof appThemeOptions[number];
export const lightEditorThemeOptions = [
  "light",
  "github",
  "one-light",
  "gothic",
  "newsprint",
  "pixyll",
  "whitey",
  "sepia",
  "solarized-light",
  "catppuccin-latte",
  "academic",
  "minimal",
  "custom"
] as const;
export type LightEditorTheme = typeof lightEditorThemeOptions[number];
export const darkEditorThemeOptions = [
  "dark",
  "github-dark",
  "one-dark",
  "one-dark-pro",
  "night",
  "solarized-dark",
  "nord",
  "catppuccin-mocha",
  "custom"
] as const;
export type DarkEditorTheme = typeof darkEditorThemeOptions[number];
export type AppThemePreferences = {
  appearanceMode: AppAppearanceMode;
  darkTheme: string;
  lightTheme: string;
};
export const defaultAppThemePreferences: AppThemePreferences = {
  appearanceMode: "system",
  darkTheme: "dark",
  lightTheme: "light"
};
export type TitlebarActionId = "viewMode" | "sourceMode" | "history" | "save" | "theme";
export type TitlebarActionPreference = {
  id: TitlebarActionId;
  visible: boolean;
};
export type ImageUploadSettings = {
  fileNamePattern: string;
};
export type PortableStoredAppSettings = {
  appearanceMode: AppAppearanceMode;
  customThemeCss: CustomThemeCssValues;
  darkTheme: string;
  editorPreferences: EditorPreferences;
  exportSettings: ExportSettings;
  fileIgnoreSettings: FileIgnoreSettings;
  language: AppLanguage;
  lightTheme: string;
};
type PortableAppSettingsPayload = Omit<PortableStoredAppSettings, "exportSettings"> & {
  exportSettings: PortableExportSettings;
};
export type StoredAppSettingsFile = {
  exportedAt: string;
  format: typeof storedAppSettingsFileFormat;
  settings: PortableAppSettingsPayload;
  version: typeof storedAppSettingsFileVersion;
};
export type ExtendedSyntaxPreferences = {
  githubAlerts: boolean;
  highlight: boolean;
};
export type EditorPreferences = {
  autoRevealActiveFile: boolean;
  autoSaveEnabled: boolean;
  autoSaveIntervalMinutes: number;
  autoUpdateEnabled: boolean;
  bodyFontSize: number;
  clipboardImageFolder: string;
  contentWidth: EditorContentWidth;
  contentWidthPx: number | null;
  documentLinksOpen: boolean;
  documentLinksVisible: boolean;
  editorFontFamily: EditorFontFamilyPreference;
  extendedSyntax: ExtendedSyntaxPreferences;
  imageUpload: ImageUploadSettings;
  lineHeight: number;
  markdownShortcuts: MarkdownShortcutBindings;
  markdownTemplates: MarkdownTemplateEntry[];
  paragraphSpacingPx: number;
  restoreWorkspaceOnStartup: boolean;
  sidebarLayoutMode: SidebarLayoutMode;
  showDocumentTabs: boolean;
  splitVisualPanePercent: number;
  tableColumnWidthMode: TableColumnWidthModePreference;
  titlebarActions: TitlebarActionPreference[];
  viewMode: ViewMode;
  viewModeCustomizations: ViewModeCustomizations;
  showLineNumbers: boolean;
  showWordCount: boolean;
  wrapCodeBlocks: boolean;
};
export type { AppLanguage };
export type { EditorContentWidth };
export type { EditorFontFamilyPreference };

export const customThemeCssMaxLength = 50000;
export const defaultCustomThemeCss = `:root[data-theme="custom"] {
  --bg-primary: #ffffff;
  --bg-secondary: #f6f8fa;
  --bg-code: #f6f8fa;
  --bg-hover: rgba(129, 139, 152, 0.1);
  --bg-active: #e6eaef;
  --text-primary: #1f2328;
  --text-heading: #1f2328;
  --text-secondary: #59636e;
  --text-md-char: #818b98;
  --border-default: #d1d9e0;
  --border-strong: #d1d9e0;
  --accent: #1a1c1e;
  --accent-soft: rgba(26, 28, 30, 0.1);
  --accent-hover: #0f1115;
}

:root[data-theme="custom"] .markdown-paper[data-editor-theme="custom"] {
  --editor-paper-bg: var(--bg-primary);
  --editor-text-primary: var(--text-primary);
  --editor-text-heading: var(--text-heading);
  --editor-text-secondary: var(--text-secondary);
  --editor-border: var(--border-default);
  --editor-border-strong: var(--border-strong);
  --editor-bg-secondary: var(--bg-secondary);
  --editor-inline-code-bg: var(--bg-code);
  --editor-code-bg: var(--bg-code);
  --editor-code-line-bg: var(--bg-secondary);
}`;
export type CustomThemeCssValues = {
  dark: string;
  light: string;
};
export const defaultCustomThemeCssValues: CustomThemeCssValues = {
  dark: defaultCustomThemeCss,
  light: defaultCustomThemeCss
};

export const defaultTitlebarActions: readonly TitlebarActionPreference[] = [
  { id: "viewMode", visible: true },
  { id: "sourceMode", visible: true },
  { id: "history", visible: true },
  { id: "save", visible: true },
  { id: "theme", visible: true }
];
export const splitVisualPanePercentMin = 25;
export const splitVisualPanePercentMax = 75;
export const defaultSplitVisualPanePercent = 50;
export const autoSaveIntervalMinutesMin = 1;
export const autoSaveIntervalMinutesMax = 120;
export const defaultAutoSaveIntervalMinutes = 10;

export const defaultImageUploadSettings: ImageUploadSettings = {
  fileNamePattern: "pasted-image-{timestamp}"
};

export const defaultExtendedSyntaxPreferences: ExtendedSyntaxPreferences = {
  githubAlerts: true,
  highlight: true
};

export const defaultEditorPreferences: EditorPreferences = {
  autoRevealActiveFile: false,
  autoSaveEnabled: true,
  autoSaveIntervalMinutes: defaultAutoSaveIntervalMinutes,
  autoUpdateEnabled: true,
  bodyFontSize: 16,
  clipboardImageFolder: "assets",
  contentWidth: "default",
  contentWidthPx: null,
  documentLinksOpen: true,
  documentLinksVisible: false,
  editorFontFamily: { ...defaultEditorFontFamily },
  extendedSyntax: { ...defaultExtendedSyntaxPreferences },
  imageUpload: defaultImageUploadSettings,
  lineHeight: 1.65,
  markdownShortcuts: defaultMarkdownShortcuts,
  markdownTemplates: [],
  paragraphSpacingPx: 8,
  restoreWorkspaceOnStartup: true,
  sidebarLayoutMode: "stacked",
  showDocumentTabs: true,
  splitVisualPanePercent: defaultSplitVisualPanePercent,
  tableColumnWidthMode: "auto",
  titlebarActions: [...defaultTitlebarActions],
  viewMode: "daily",
  viewModeCustomizations: { ...defaultViewModeCustomizations },
  showLineNumbers: false,
  showWordCount: true,
  wrapCodeBlocks: true
};

const editorBodyFontSizeOptions = [14, 15, 16, 17, 18, 20] as const;
const editorLineHeightOptions = [1.5, 1.65, 1.8] as const;
export const editorParagraphSpacingPxMin = 0;
export const editorParagraphSpacingPxMax = 32;
const sidebarLayoutModeOptions: readonly SidebarLayoutMode[] = ["stacked", "tabs"];
const tableColumnWidthModeOptions: readonly TableColumnWidthModePreference[] = ["even", "auto"];

function loadSettingsStore() {
  return getAppRuntime().settings.loadStore(settingsStorePath, { autoSave: false, defaults: {} });
}

async function readSettingsGroup<TValue>(group: import("../../runtime").AppSettingsGroup) {
  return getAppRuntime().settings.readGroup?.<TValue>(group);
}

async function writeSettingsGroup(
  group: import("../../runtime").AppSettingsGroup,
  value: unknown
) {
  const writeGroup = getAppRuntime().settings.writeGroup;
  if (!writeGroup) return false;

  await writeGroup(group, value);
  return true;
}

function isSettingsRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function invalidStoredAppSettingsFile(): Error {
  return new Error(invalidStoredAppSettingsFileMessage);
}

function parseStoredAppSettingsFile(contents: string): PortableAppSettingsPayload {
  let parsed: unknown;
  try {
    parsed = JSON.parse(contents);
  } catch {
    throw invalidStoredAppSettingsFile();
  }

  if (!isSettingsRecord(parsed)) throw invalidStoredAppSettingsFile();
  if (parsed.format !== storedAppSettingsFileFormat) throw invalidStoredAppSettingsFile();
  if (
    parsed.version !== 1 &&
    parsed.version !== 2 &&
    parsed.version !== storedAppSettingsFileVersion
  ) throw invalidStoredAppSettingsFile();
  if (!isSettingsRecord(parsed.settings)) throw invalidStoredAppSettingsFile();

  return normalizePortableStoredAppSettings(parsed.settings);
}

function normalizePortableStoredAppSettings(value: Record<string, unknown>): PortableAppSettingsPayload {
  const themePreferences = normalizeAppThemePreferences(value);

  return {
    appearanceMode: themePreferences.appearanceMode,
    customThemeCss: normalizeCustomThemeCssValues(value.customThemeCss),
    darkTheme: themePreferences.darkTheme,
    editorPreferences: normalizeEditorPreferences(value.editorPreferences),
    exportSettings: normalizePortableExportSettings(value.exportSettings),
    fileIgnoreSettings: normalizeFileIgnoreSettings(value.fileIgnoreSettings),
    language: isAppLanguage(value.language) ? value.language : "en",
    lightTheme: themePreferences.lightTheme
  };
}

async function readPortableStoredAppSettings(): Promise<PortableAppSettingsPayload> {
  const store = await loadSettingsStore();
  const legacyTheme = await store.get<AppTheme>(themeKey);
  const legacyPreferences = isAppTheme(legacyTheme)
    ? createThemePreferencesFromLegacyTheme(legacyTheme)
    : defaultAppThemePreferences;
  const themePreferences = normalizeAppThemePreferences({
    appearanceMode: await store.get<AppAppearanceMode>(appearanceModeKey),
    darkTheme: await store.get<string>(darkThemeKey) ?? await store.get<string>(legacyDarkThemeKey),
    lightTheme: await store.get<string>(lightThemeKey) ?? await store.get<string>(legacyLightThemeKey)
  }, legacyPreferences);
  const legacyCustomThemeCss = normalizeCustomThemeCss(await store.get<string>(customThemeCssKey));
  const customThemeCss = normalizeCustomThemeCssValues({
    dark: await store.get<string>(darkCustomThemeCssKey) ?? legacyCustomThemeCss,
    light: await store.get<string>(lightCustomThemeCssKey) ?? legacyCustomThemeCss
  });
  const language = await store.get<AppLanguage>(languageKey);
  const editorPreferences = await store.get<Partial<EditorPreferences>>(editorPreferencesKey);
  const exportSettings = await store.get<Partial<PortableExportSettings>>(exportSettingsKey);
  const fileIgnoreSettings = await store.get<Partial<FileIgnoreSettings>>(fileIgnoreSettingsKey);

  return {
    appearanceMode: themePreferences.appearanceMode,
    customThemeCss,
    darkTheme: themePreferences.darkTheme,
    editorPreferences: normalizeEditorPreferences(editorPreferences),
    exportSettings: normalizePortableExportSettings(exportSettings),
    fileIgnoreSettings: normalizeFileIgnoreSettings(fileIgnoreSettings),
    language: isAppLanguage(language) ? language : "en",
    lightTheme: themePreferences.lightTheme
  };
}

async function writePortableStoredAppSettings(settings: PortableAppSettingsPayload) {
  const runtime = getAppRuntime();
  if (runtime.settings.replacePortable) {
    await runtime.settings.replacePortable(settings);
    return;
  }
  const store = await loadSettingsStore();

  await store.set(appearanceModeKey, settings.appearanceMode);
  await store.set(darkCustomThemeCssKey, settings.customThemeCss.dark);
  await store.set(darkThemeKey, settings.darkTheme);
  await store.set(editorPreferencesKey, settings.editorPreferences);
  await store.set(exportSettingsKey, settings.exportSettings);
  await store.set(fileIgnoreSettingsKey, settings.fileIgnoreSettings);
  await store.set(languageKey, settings.language);
  await store.set(lightCustomThemeCssKey, settings.customThemeCss.light);
  await store.set(lightThemeKey, settings.lightTheme);
  await store.save();
}

export async function exportStoredAppSettings(exportedAt: Date = new Date()) {
  const settingsFile: StoredAppSettingsFile = {
    exportedAt: exportedAt.toISOString(),
    format: storedAppSettingsFileFormat,
    settings: await readPortableStoredAppSettings(),
    version: storedAppSettingsFileVersion
  };

  return JSON.stringify(settingsFile, null, 2);
}

export async function importStoredAppSettings(contents: string) {
  const settings = parseStoredAppSettingsFile(contents);

  await writePortableStoredAppSettings(settings);

  return {
    ...settings,
    exportSettings: normalizeExportSettings({
      ...settings.exportSettings,
      pandocPath: await loadLocalPandocPath()
    })
  };
}

export function isAppTheme(value: unknown): value is AppTheme {
  return appThemeOptions.includes(value as AppTheme);
}

export function isEditorTheme(value: unknown): value is EditorTheme {
  return editorThemeOptions.includes(value as typeof editorThemeOptions[number]);
}

export function isAppAppearanceMode(value: unknown): value is AppAppearanceMode {
  return appAppearanceModeOptions.includes(value as AppAppearanceMode);
}

export function isLightEditorTheme(value: unknown): value is LightEditorTheme {
  return lightEditorThemeOptions.includes(value as LightEditorTheme);
}

export function isDarkEditorTheme(value: unknown): value is DarkEditorTheme {
  return darkEditorThemeOptions.includes(value as DarkEditorTheme);
}

export function isThemeId(value: unknown): value is string {
  if (typeof value !== "string" || value.length < 1 || value.length > 64) return false;
  if (!/^[a-z0-9][a-z0-9-]*$/u.test(value)) return false;

  return value === "light" || value === "dark" || !value.startsWith("qingyu-");
}

export function normalizeCustomThemeCss(value: unknown) {
  if (typeof value !== "string") return defaultCustomThemeCss;

  return value.slice(0, customThemeCssMaxLength);
}

export function normalizeCustomThemeCssValues(value: unknown): CustomThemeCssValues {
  if (typeof value !== "object" || value === null) {
    const css = normalizeCustomThemeCss(value);

    return {
      dark: css,
      light: css
    };
  }

  const css = value as Partial<Record<keyof CustomThemeCssValues, unknown>>;

  return {
    dark: normalizeCustomThemeCss(css.dark),
    light: normalizeCustomThemeCss(css.light)
  };
}

export function normalizeAppThemePreferences(
  value: unknown,
  fallback: AppThemePreferences = defaultAppThemePreferences
): AppThemePreferences {
  if (typeof value !== "object" || value === null) return fallback;

  const preferences = value as Partial<Record<keyof AppThemePreferences, unknown>>;

  return {
    appearanceMode: isAppAppearanceMode(preferences.appearanceMode)
      ? preferences.appearanceMode
      : fallback.appearanceMode,
    darkTheme: isThemeId(preferences.darkTheme) ? preferences.darkTheme : fallback.darkTheme,
    lightTheme: isThemeId(preferences.lightTheme) ? preferences.lightTheme : fallback.lightTheme
  };
}

export function resolveAppAppearanceTheme(theme: AppTheme, systemTheme: ResolvedAppTheme): ResolvedAppTheme {
  if (theme === "system") return systemTheme;
  if (isDarkEditorTheme(theme) && theme !== "custom") return "dark";

  return "light";
}

export function resolveAppEditorTheme(theme: AppTheme, systemTheme: ResolvedAppTheme): EditorTheme {
  if (theme === "system") return systemTheme;

  return theme;
}

export function createThemePreferencesFromLegacyTheme(theme: AppTheme): AppThemePreferences {
  if (theme === "system") return defaultAppThemePreferences;
  if (isDarkEditorTheme(theme) && theme !== "custom") {
    return {
      ...defaultAppThemePreferences,
      appearanceMode: "dark",
      darkTheme: theme
    };
  }
  if (isLightEditorTheme(theme)) {
    return {
      ...defaultAppThemePreferences,
      appearanceMode: "light",
      lightTheme: theme
    };
  }

  return defaultAppThemePreferences;
}

export function resolveAppThemePreferencesAppearance(
  preferences: AppThemePreferences,
  systemTheme: ResolvedAppTheme
): ResolvedAppTheme {
  return preferences.appearanceMode === "system" ? systemTheme : preferences.appearanceMode;
}

export function resolveAppThemePreferencesEditorTheme(
  preferences: AppThemePreferences,
  systemTheme: ResolvedAppTheme
): string {
  return resolveAppThemePreferencesAppearance(preferences, systemTheme) === "dark"
    ? preferences.darkTheme
    : preferences.lightTheme;
}

export async function getStoredTheme(): Promise<AppTheme> {
  const store = await loadSettingsStore();
  const theme = await store.get<AppTheme>(themeKey);

  return isAppTheme(theme) ? theme : "system";
}

export async function saveStoredTheme(theme: AppTheme) {
  const store = await loadSettingsStore();

  await store.set(themeKey, theme);
  await store.save();
}

export async function getStoredThemePreferences(): Promise<AppThemePreferences> {
  const grouped = await readSettingsGroup<AppThemePreferences>("appearance");
  if (grouped !== undefined && grouped !== null) {
    return normalizeAppThemePreferences(grouped);
  }
  const store = await loadSettingsStore();
  const appearanceMode = await store.get<AppAppearanceMode>(appearanceModeKey);
  const lightTheme = await store.get<string>(lightThemeKey) ?? await store.get<string>(legacyLightThemeKey);
  const darkTheme = await store.get<string>(darkThemeKey) ?? await store.get<string>(legacyDarkThemeKey);
  const legacyTheme = await store.get<AppTheme>(themeKey);
  const legacyPreferences = isAppTheme(legacyTheme)
    ? createThemePreferencesFromLegacyTheme(legacyTheme)
    : defaultAppThemePreferences;

  return {
    appearanceMode: isAppAppearanceMode(appearanceMode) ? appearanceMode : legacyPreferences.appearanceMode,
    darkTheme: isThemeId(darkTheme) ? darkTheme : legacyPreferences.darkTheme,
    lightTheme: isThemeId(lightTheme) ? lightTheme : legacyPreferences.lightTheme
  };
}

export async function saveStoredThemePreferences(preferences: AppThemePreferences) {
  const normalizedPreferences = normalizeAppThemePreferences(preferences);
  if (await writeSettingsGroup("appearance", normalizedPreferences)) return;
  const store = await loadSettingsStore();

  await store.set(appearanceModeKey, normalizedPreferences.appearanceMode);
  await store.set(lightThemeKey, normalizedPreferences.lightTheme);
  await store.set(darkThemeKey, normalizedPreferences.darkTheme);
  await store.save();
}

export async function getApprovedThemeFingerprint(id: string) {
  if (!isThemeId(id)) return null;
  const store = await loadSettingsStore();
  const approvals = await store.get<unknown>(approvedThemeFingerprintsKey);
  if (typeof approvals !== "object" || approvals === null || Array.isArray(approvals)) return null;
  const fingerprint = (approvals as Record<string, unknown>)[id];

  return typeof fingerprint === "string" && /^[a-f0-9]{64}$/u.test(fingerprint) ? fingerprint : null;
}

export async function approveThemeFingerprint(id: string, fingerprint: string) {
  if (!isThemeId(id) || !/^[a-f0-9]{64}$/u.test(fingerprint)) return;
  const store = await loadSettingsStore();
  const stored = await store.get<unknown>(approvedThemeFingerprintsKey);
  const approvals = typeof stored === "object" && stored !== null && !Array.isArray(stored)
    ? { ...(stored as Record<string, unknown>) }
    : {};
  const normalized = Object.fromEntries(
    Object.entries(approvals)
      .filter(([themeId, value]) => isThemeId(themeId) && typeof value === "string" && /^[a-f0-9]{64}$/u.test(value))
      .slice(-99)
  );
  normalized[id] = fingerprint;
  await store.set(approvedThemeFingerprintsKey, normalized);
  await store.save();
}

export async function getStoredCustomThemeCss() {
  const grouped = await readSettingsGroup<CustomThemeCssValues>("customThemeCss");
  if (grouped !== undefined && grouped !== null) return normalizeCustomThemeCssValues(grouped);
  const store = await loadSettingsStore();
  const lightCss = await store.get<string>(lightCustomThemeCssKey);
  const darkCss = await store.get<string>(darkCustomThemeCssKey);
  const legacyCss = await store.get<string>(customThemeCssKey);
  const fallbackCss = normalizeCustomThemeCss(legacyCss);

  return {
    dark: typeof darkCss === "string" ? normalizeCustomThemeCss(darkCss) : fallbackCss,
    light: typeof lightCss === "string" ? normalizeCustomThemeCss(lightCss) : fallbackCss
  };
}

export async function saveStoredCustomThemeCss(css: CustomThemeCssValues) {
  const normalizedCss = normalizeCustomThemeCssValues(css);
  if (await writeSettingsGroup("customThemeCss", normalizedCss)) return;
  const store = await loadSettingsStore();

  await store.set(lightCustomThemeCssKey, normalizedCss.light);
  await store.set(darkCustomThemeCssKey, normalizedCss.dark);
  await store.save();
}

export async function forgetApprovedThemeFingerprint(id: string) {
  const store = await loadSettingsStore();
  const stored = await store.get<unknown>(approvedThemeFingerprintsKey);
  if (typeof stored !== "object" || stored === null || Array.isArray(stored)) return;
  const approvals = { ...(stored as Record<string, unknown>) };
  delete approvals[id];
  await store.set(approvedThemeFingerprintsKey, approvals);
  await store.save();
}

export async function getStoredLanguage(): Promise<AppLanguage> {
  const grouped = await readSettingsGroup<AppLanguage>("language");
  if (isAppLanguage(grouped)) return grouped;
  const store = await loadSettingsStore();
  const language = await store.get<AppLanguage>(languageKey);

  return isAppLanguage(language) ? language : "en";
}

export async function saveStoredLanguage(language: AppLanguage) {
  if (await writeSettingsGroup("language", language)) return;
  const store = await loadSettingsStore();

  await store.set(languageKey, language);
  await store.save();
}

export async function getStoredEditorPreferences(): Promise<EditorPreferences> {
  const grouped = await readSettingsGroup<Partial<EditorPreferences>>("editorPreferences");
  if (grouped !== undefined && grouped !== null) return normalizeEditorPreferences(grouped);
  const store = await loadSettingsStore();
  const preferences = await store.get<Partial<EditorPreferences>>(editorPreferencesKey);

  return normalizeEditorPreferences(preferences);
}

export async function saveStoredEditorPreferences(preferences: EditorPreferences) {
  const normalized = normalizeEditorPreferences(preferences);
  if (await writeSettingsGroup("editorPreferences", normalized)) return;
  const store = await loadSettingsStore();

  await store.set(editorPreferencesKey, normalized);
  await store.save();
}

export async function getStoredFileIgnoreSettings(): Promise<FileIgnoreSettings> {
  const grouped = await readSettingsGroup<Partial<FileIgnoreSettings>>("fileIgnoreSettings");
  if (grouped !== undefined && grouped !== null) return normalizeFileIgnoreSettings(grouped);
  const store = await loadSettingsStore();
  const settings = await store.get<Partial<FileIgnoreSettings>>(fileIgnoreSettingsKey);

  return normalizeFileIgnoreSettings(settings ?? defaultFileIgnoreSettings);
}

export async function saveStoredFileIgnoreSettings(settings: FileIgnoreSettings) {
  const normalized = normalizeFileIgnoreSettings(settings);
  if (await writeSettingsGroup("fileIgnoreSettings", normalized)) return;
  const store = await loadSettingsStore();

  await store.set(fileIgnoreSettingsKey, normalized);
  await store.save();
}

export async function getStoredExportSettings(): Promise<ExportSettings> {
  const grouped = await readSettingsGroup<Partial<PortableExportSettings>>("exportSettings");
  const portableSettings = grouped !== undefined && grouped !== null
    ? normalizePortableExportSettings(grouped)
    : normalizePortableExportSettings(
        await (await loadSettingsStore()).get<Partial<PortableExportSettings>>(exportSettingsKey)
      );
  const pandocPath = await loadLocalPandocPath();

  return normalizeExportSettings({ ...portableSettings, pandocPath });
}

export async function saveStoredExportSettings(settings: ExportSettings) {
  const normalized = normalizeExportSettings(settings);
  const portable = normalizePortableExportSettings(normalized);
  await saveLocalPandocPath(normalized.pandocPath);
  if (await writeSettingsGroup("exportSettings", portable)) return;
  const store = await loadSettingsStore();

  await store.set(exportSettingsKey, portable);
  await store.save();
}

function normalizeSidebarLayoutMode(value: unknown): SidebarLayoutMode {
  return sidebarLayoutModeOptions.includes(value as SidebarLayoutMode)
    ? (value as SidebarLayoutMode)
    : defaultEditorPreferences.sidebarLayoutMode;
}

function normalizeAutoSaveIntervalMinutes(value: unknown) {
  const clamped = clampNumber(value, autoSaveIntervalMinutesMin, autoSaveIntervalMinutesMax);
  if (clamped === null) return defaultAutoSaveIntervalMinutes;

  return Math.round(clamped);
}

function normalizeTableColumnWidthMode(value: unknown): TableColumnWidthModePreference {
  return tableColumnWidthModeOptions.includes(value as TableColumnWidthModePreference)
    ? (value as TableColumnWidthModePreference)
    : defaultEditorPreferences.tableColumnWidthMode;
}

export function normalizeEditorPreferences(value: unknown): EditorPreferences {
  if (typeof value !== "object" || value === null) {
    return {
      ...defaultEditorPreferences,
      editorFontFamily: { ...defaultEditorFontFamily },
      extendedSyntax: { ...defaultExtendedSyntaxPreferences },
      titlebarActions: [...defaultTitlebarActions],
      viewModeCustomizations: { ...defaultViewModeCustomizations }
    };
  }

  const preferences = value as Partial<EditorPreferences>;

  return {
    autoRevealActiveFile:
      typeof preferences.autoRevealActiveFile === "boolean"
        ? preferences.autoRevealActiveFile
        : defaultEditorPreferences.autoRevealActiveFile,
    autoSaveEnabled:
      typeof preferences.autoSaveEnabled === "boolean"
        ? preferences.autoSaveEnabled
        : defaultEditorPreferences.autoSaveEnabled,
    autoSaveIntervalMinutes: normalizeAutoSaveIntervalMinutes(preferences.autoSaveIntervalMinutes),
    autoUpdateEnabled:
      typeof preferences.autoUpdateEnabled === "boolean"
        ? preferences.autoUpdateEnabled
        : defaultEditorPreferences.autoUpdateEnabled,
    bodyFontSize: editorBodyFontSizeOptions.includes(preferences.bodyFontSize as typeof editorBodyFontSizeOptions[number])
      ? Number(preferences.bodyFontSize)
      : defaultEditorPreferences.bodyFontSize,
    clipboardImageFolder: normalizeClipboardImageFolder(preferences.clipboardImageFolder),
    contentWidth: editorContentWidthOptions.includes(preferences.contentWidth as EditorContentWidth)
      ? (preferences.contentWidth as EditorContentWidth)
      : defaultEditorPreferences.contentWidth,
    contentWidthPx: normalizeEditorContentWidthPx(preferences.contentWidthPx),
    documentLinksOpen:
      typeof preferences.documentLinksOpen === "boolean"
        ? preferences.documentLinksOpen
        : defaultEditorPreferences.documentLinksOpen,
    documentLinksVisible:
      typeof preferences.documentLinksVisible === "boolean"
        ? preferences.documentLinksVisible
        : defaultEditorPreferences.documentLinksVisible,
    editorFontFamily: normalizeEditorFontFamilyPreference(preferences.editorFontFamily),
    extendedSyntax: normalizeExtendedSyntaxPreferences(preferences.extendedSyntax),
    imageUpload: normalizeImageUploadSettings(preferences.imageUpload),
    lineHeight: editorLineHeightOptions.includes(preferences.lineHeight as typeof editorLineHeightOptions[number])
      ? Number(preferences.lineHeight)
      : defaultEditorPreferences.lineHeight,
    markdownShortcuts: normalizeMarkdownShortcuts(preferences.markdownShortcuts),
    markdownTemplates: normalizeMarkdownTemplateEntries(preferences.markdownTemplates),
    paragraphSpacingPx: normalizeEditorParagraphSpacingPx(preferences.paragraphSpacingPx),
    restoreWorkspaceOnStartup:
      typeof preferences.restoreWorkspaceOnStartup === "boolean"
        ? preferences.restoreWorkspaceOnStartup
        : defaultEditorPreferences.restoreWorkspaceOnStartup,
    sidebarLayoutMode: normalizeSidebarLayoutMode(preferences.sidebarLayoutMode),
    showDocumentTabs:
      typeof preferences.showDocumentTabs === "boolean"
        ? preferences.showDocumentTabs
        : defaultEditorPreferences.showDocumentTabs,
    splitVisualPanePercent: normalizeSplitVisualPanePercent(preferences.splitVisualPanePercent),
    tableColumnWidthMode: normalizeTableColumnWidthMode(preferences.tableColumnWidthMode),
    titlebarActions: normalizeTitlebarActions(preferences.titlebarActions),
    viewMode: isViewMode(preferences.viewMode) ? preferences.viewMode : defaultEditorPreferences.viewMode,
    viewModeCustomizations: normalizeViewModeCustomizations(preferences.viewModeCustomizations),
    showLineNumbers:
      typeof preferences.showLineNumbers === "boolean"
        ? preferences.showLineNumbers
        : defaultEditorPreferences.showLineNumbers,
    showWordCount:
      typeof preferences.showWordCount === "boolean" ? preferences.showWordCount : defaultEditorPreferences.showWordCount,
    wrapCodeBlocks:
      typeof preferences.wrapCodeBlocks === "boolean" ? preferences.wrapCodeBlocks : defaultEditorPreferences.wrapCodeBlocks
  };
}

function normalizeEditorParagraphSpacingPx(value: unknown) {
  if (typeof value !== "number" || !Number.isFinite(value)) return defaultEditorPreferences.paragraphSpacingPx;

  return Math.min(editorParagraphSpacingPxMax, Math.max(editorParagraphSpacingPxMin, Math.round(value)));
}

function normalizeExtendedSyntaxPreferences(value: unknown): ExtendedSyntaxPreferences {
  if (typeof value !== "object" || value === null) {
    return { ...defaultExtendedSyntaxPreferences };
  }

  const preferences = value as Partial<ExtendedSyntaxPreferences>;

  return {
    githubAlerts:
      typeof preferences.githubAlerts === "boolean"
        ? preferences.githubAlerts
        : defaultExtendedSyntaxPreferences.githubAlerts,
    highlight:
      typeof preferences.highlight === "boolean"
        ? preferences.highlight
        : defaultExtendedSyntaxPreferences.highlight
  };
}

export function normalizeSplitVisualPanePercent(value: unknown) {
  const percent = clampNumber(value, splitVisualPanePercentMin, splitVisualPanePercentMax);
  if (percent === null) return defaultSplitVisualPanePercent;

  return Math.round(percent);
}

export function normalizeTitlebarActions(value: unknown): TitlebarActionPreference[] {
  if (!Array.isArray(value)) return [...defaultTitlebarActions];

  const knownIds = new Set<TitlebarActionId>(defaultTitlebarActions.map((action) => action.id));
  const usedIds = new Set<TitlebarActionId>();
  const normalized: TitlebarActionPreference[] = [];

  value.forEach((item) => {
    const candidate = typeof item === "object" && item !== null ? item as Partial<TitlebarActionPreference> : null;
    const id = candidate?.id;
    if (!id || !knownIds.has(id) || usedIds.has(id)) return;

    usedIds.add(id);
    normalized.push({
      id,
      visible: typeof candidate.visible === "boolean" ? candidate.visible : true
    });
  });

  defaultTitlebarActions.forEach((action) => {
    if (usedIds.has(action.id)) return;

    normalized.push({ ...action });
  });

  return normalized;
}

export function reorderTitlebarActions(
  actions: readonly TitlebarActionPreference[],
  draggedId: TitlebarActionId,
  targetId: TitlebarActionId
): TitlebarActionPreference[] {
  const normalized = normalizeTitlebarActions(actions);
  if (draggedId === targetId) return normalized;

  const fromIndex = normalized.findIndex((action) => action.id === draggedId);
  const toIndex = normalized.findIndex((action) => action.id === targetId);
  if (fromIndex < 0 || toIndex < 0) return normalized;

  const draggedAction = normalized[fromIndex];
  const nextActions = normalized.filter((action) => action.id !== draggedId);

  nextActions.splice(toIndex, 0, draggedAction);

  return nextActions;
}

export function normalizeImageUploadSettings(value: unknown): ImageUploadSettings {
  if (typeof value !== "object" || value === null) return defaultImageUploadSettings;

  const settings = value as Partial<ImageUploadSettings>;

  return {
    fileNamePattern: normalizeImageUploadFileNamePattern(settings.fileNamePattern)
  };
}

export function normalizeImageUploadFileNamePattern(value: unknown) {
  if (typeof value !== "string") return defaultImageUploadSettings.fileNamePattern;

  const pattern = value.trim();
  if (!pattern || pattern.includes("/") || pattern.includes("\\") || pattern === "." || pattern === "..") {
    return defaultImageUploadSettings.fileNamePattern;
  }

  return pattern.slice(0, 120);
}

export function normalizeClipboardImageFolder(value: unknown) {
  if (typeof value !== "string") return defaultEditorPreferences.clipboardImageFolder;
  if (/\p{Cc}/u.test(value)) return defaultEditorPreferences.clipboardImageFolder;

  const normalized = value.trim().replace(/\\/gu, "/").replace(/\/+/gu, "/");
  if (normalized === ".") return ".";
  if (!normalized || normalized.startsWith("/") || /^[a-zA-Z]:/u.test(normalized)) {
    return defaultEditorPreferences.clipboardImageFolder;
  }

  const parts = normalized
    .split("/")
    .map((part) => part.trim())
    .filter((part) => part.length > 0 && part !== ".");

  if (!parts.length || parts.some((part) => part === "..")) {
    return defaultEditorPreferences.clipboardImageFolder;
  }

  return parts.join("/");
}
