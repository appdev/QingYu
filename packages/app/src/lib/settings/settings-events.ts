import {
  isAppTheme,
  getStoredExportSettings,
  normalizeAppThemePreferences,
  normalizeCustomThemeCss,
  normalizeCustomThemeCssValues,
  normalizeEditorPreferences,
  normalizeFileIgnoreSettings,
  createThemePreferencesFromLegacyTheme,
  type AppThemePreferences,
  type CustomThemeCssValues,
  type EditorPreferences,
  type ExportSettings,
  type FileIgnoreSettings
} from "./app-settings";
import { isAppLanguage, type AppLanguage } from "@markra/shared";
import { getAppRuntime } from "../../runtime";
import { normalizeMcpConfig, type McpConfig } from "../mcp";

const themeChangedEvent = "markra://theme-changed";
const customThemeCssChangedEvent = "markra://custom-theme-css-changed";
const languageChangedEvent = "markra://language-changed";
const editorPreferencesChangedEvent = "markra://editor-preferences-changed";
const exportSettingsChangedEvent = "markra://export-settings-changed";
const fileIgnoreSettingsChangedEvent = "markra://file-ignore-settings-changed";
const themeCatalogChangedEvent = "markra://theme-catalog-changed";
const mcpPolicyChangedEvent = "qingyu://settings-mcp-changed";
const mcpRuntimeChangedEvent = "qingyu://mcp-runtime-changed";
const settingsEventsSourceId =
  globalThis.crypto?.randomUUID?.() ?? `markra-settings-${Math.random().toString(36).slice(2)}`;

type ThemeChangedPayload = {
  preferences?: AppThemePreferences;
  theme?: unknown;
};

type CustomThemeCssChangedPayload = {
  css?: unknown;
  customThemeCss?: CustomThemeCssValues;
};

type LanguageChangedPayload = {
  language: AppLanguage;
};

type EditorPreferencesChangedPayload = {
  preferences: EditorPreferences;
  sourceId?: string;
};

type ExportSettingsChangedPayload = {
  settings: ExportSettings;
};

type FileIgnoreSettingsChangedPayload = {
  settings: FileIgnoreSettings;
};

type McpPolicyChangedPayload = {
  config: McpConfig;
};

type McpRuntimeChangedPayload = {
  workspaceGeneration: number;
};

function isEditorPreferencesPayload(value: unknown) {
  if (typeof value !== "object" || value === null) return false;

  const preferences = value as Record<string, unknown>;

  return typeof preferences.bodyFontSize === "number";
}

export async function notifyAppThemeChanged(preferences: AppThemePreferences) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(themeChangedEvent, { preferences: normalizeAppThemePreferences(preferences) });
}

export async function listenAppThemeChanged(onThemeChanged: (preferences: AppThemePreferences) => unknown) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<ThemeChangedPayload>(themeChangedEvent, (event) => {
    if (event.payload.preferences) {
      onThemeChanged(normalizeAppThemePreferences(event.payload.preferences));
      return;
    }

    if (isAppTheme(event.payload.theme)) {
      onThemeChanged(createThemePreferencesFromLegacyTheme(event.payload.theme));
    }
  });
}

export async function notifyAppCustomThemeCssChanged(customThemeCss: CustomThemeCssValues) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(customThemeCssChangedEvent, {
    customThemeCss: normalizeCustomThemeCssValues(customThemeCss)
  });
}

export async function listenAppCustomThemeCssChanged(onCustomThemeCssChanged: (css: CustomThemeCssValues) => unknown) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<CustomThemeCssChangedPayload>(customThemeCssChangedEvent, (event) => {
    if (event.payload.customThemeCss) {
      onCustomThemeCssChanged(normalizeCustomThemeCssValues(event.payload.customThemeCss));
      return;
    }

    if (typeof event.payload.css === "string") {
      const css = normalizeCustomThemeCss(event.payload.css);

      onCustomThemeCssChanged({ dark: css, light: css });
    }
  });
}

export async function notifyThemeCatalogChanged() {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(themeCatalogChangedEvent, {
    revision: globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`
  });
}

export async function listenThemeCatalogChanged(onCatalogChanged: () => unknown) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<{ revision: string }>(themeCatalogChangedEvent, () => {
    onCatalogChanged();
  });
}

export async function notifyAppLanguageChanged(language: AppLanguage) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(languageChangedEvent, { language });
}

export async function listenAppLanguageChanged(onLanguageChanged: (language: AppLanguage) => unknown) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<LanguageChangedPayload>(languageChangedEvent, (event) => {
    if (isAppLanguage(event.payload.language)) {
      onLanguageChanged(event.payload.language);
    }
  });
}

export async function notifyAppEditorPreferencesChanged(preferences: EditorPreferences) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(editorPreferencesChangedEvent, { preferences, sourceId: settingsEventsSourceId });
}

export async function listenAppEditorPreferencesChanged(
  onPreferencesChanged: (preferences: EditorPreferences) => unknown
) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<EditorPreferencesChangedPayload>(editorPreferencesChangedEvent, (event) => {
    // Local state was already updated before emitting; replaying our own normalized event can erase in-progress input.
    if (event.payload.sourceId === settingsEventsSourceId) return;

    if (isEditorPreferencesPayload(event.payload.preferences)) {
      onPreferencesChanged(normalizeEditorPreferences(event.payload.preferences));
    }
  });
}

export async function notifyAppExportSettingsChanged(settings: ExportSettings) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(exportSettingsChangedEvent, { settings });
}

export async function listenAppExportSettingsChanged(
  onSettingsChanged: (settings: ExportSettings) => unknown
) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<ExportSettingsChangedPayload>(exportSettingsChangedEvent, async () => {
    onSettingsChanged(await getStoredExportSettings());
  });
}

export async function notifyAppFileIgnoreSettingsChanged(settings: FileIgnoreSettings) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(fileIgnoreSettingsChangedEvent, {
    settings: normalizeFileIgnoreSettings(settings)
  });
}

export async function listenAppFileIgnoreSettingsChanged(
  onSettingsChanged: (settings: FileIgnoreSettings) => unknown
) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<FileIgnoreSettingsChangedPayload>(
    fileIgnoreSettingsChangedEvent,
    (event) => {
      onSettingsChanged(normalizeFileIgnoreSettings(event.payload.settings));
    }
  );
}

export async function notifyMcpPolicyChanged(config: McpConfig) {
  if (!getAppRuntime().events.isAvailable()) return;

  await getAppRuntime().events.emit(mcpPolicyChangedEvent, {
    config: normalizeMcpConfig(config)
  });
}

export async function listenMcpPolicyChanged(onPolicyChanged: (config: McpConfig) => unknown) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<McpPolicyChangedPayload>(mcpPolicyChangedEvent, (event) => {
    onPolicyChanged(normalizeMcpConfig(event.payload.config));
  });
}

export async function listenMcpRuntimeChanged(onRuntimeChanged: () => unknown) {
  if (!getAppRuntime().events.isAvailable()) return () => {};

  return getAppRuntime().events.listen<McpRuntimeChangedPayload>(mcpRuntimeChangedEvent, () => {
    onRuntimeChanged();
  });
}
