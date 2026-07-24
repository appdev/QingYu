import type { AppLanguage } from "@markra/shared";
import type { AppFeatureRuntime } from "../../runtime";
import type { DesktopPlatform } from "../platform";
import type { EditorPreferences, ExportSettings } from "../settings/app-settings";

export type DiagnosticsReportInput = {
  appVersion: string;
  editorPreferences: EditorPreferences;
  exportSettings: ExportSettings;
  features: AppFeatureRuntime;
  generatedAt?: Date;
  language: AppLanguage;
  osVersion: string | null;
  platform: DesktopPlatform | null;
};

export type CrashDiagnosticsReportInput = {
  appVersion: string;
  componentStack?: string | null;
  error: unknown;
  generatedAt?: Date;
  language: AppLanguage;
  osVersion: string | null;
  platform: DesktopPlatform | null;
};

const diagnosticsIssueUrl = "https://github.com/appdev/QingYu/issues/new";

export function generateDiagnosticsReport({
  appVersion,
  editorPreferences,
  exportSettings,
  features,
  generatedAt = new Date(),
  language,
  osVersion,
  platform
}: DiagnosticsReportInput) {
  const reportTime = Number.isFinite(generatedAt.getTime()) ? generatedAt.toISOString() : "unknown";

  return [
    "## QingYu Diagnostics",
    "",
    "### App",
    line("Report time", reportTime),
    line("App version", appVersion || "unknown"),
    line("Platform", platform ?? "unknown"),
    line("OS version", osVersion?.trim() || "unknown"),
    line("App language", language),
    "",
    "### Features",
    line("Application menu enabled", features.applicationMenu),
    line("Application shortcuts enabled", features.applicationShortcuts),
    line("Export feature enabled", features.export),
    line("File drop enabled", features.fileDrop),
    line("Image import enabled", features.imageImport),
    line("Native window chrome enabled", features.nativeWindowChrome),
    line("Local attachment opening enabled", features.openLocalAttachments),
    line("Pandoc feature enabled", features.pandoc),
    line("Primary notes sync enabled", features.projectSync),
    line("Settings window enabled", features.settingsWindow),
    line("System fonts enabled", features.systemFonts),
    line("Updater feature enabled", features.updater),
    "",
    "### Editor",
    line("Restore workspace on startup", editorPreferences.restoreWorkspaceOnStartup),
    line("Auto-save enabled", editorPreferences.autoSaveEnabled),
    line("Auto-save interval", minuteBucket(editorPreferences.autoSaveIntervalMinutes)),
    line("View mode", editorPreferences.viewMode),
    line("Document tabs enabled", editorPreferences.showDocumentTabs),
    "",
    "### Export",
    line("PDF page size", exportSettings.pdfPageSize),
    line("PDF margin preset", exportSettings.pdfMarginPreset),
    line("PDF page break on H1", exportSettings.pdfPageBreakOnH1),
    line("Pandoc path configured", exportSettings.pandocPath.trim().length > 0),
    line("Pandoc args configured", exportSettings.pandocArgs.trim().length > 0),
    "",
    "### Privacy",
    "- This report is generated locally.",
    "- It does not include document contents, file names, file paths, credentials, endpoint URLs, or raw logs."
  ].join("\n");
}

export function generateCrashDiagnosticsReport({
  appVersion,
  componentStack,
  error,
  generatedAt = new Date(),
  language,
  osVersion,
  platform
}: CrashDiagnosticsReportInput) {
  const reportTime = Number.isFinite(generatedAt.getTime()) ? generatedAt.toISOString() : "unknown";
  const normalizedError = normalizeCrashError(error);

  return [
    "## QingYu Crash Report",
    "",
    "### Error",
    line("Error name", normalizedError.name),
    line("Error message", normalizedError.message),
    line("Component stack", componentStack?.trim() ? "available" : "unavailable"),
    "",
    "### App",
    line("Report time", reportTime),
    line("App version", appVersion || "unknown"),
    line("Platform", platform ?? "unknown"),
    line("OS version", osVersion?.trim() || "unknown"),
    line("App language", language),
    "",
    "### Privacy",
    "- This report is generated locally.",
    "- It does not include document contents, file names, file paths, credentials, endpoint URLs, raw logs, or raw JavaScript stacks.",
    "- Please review the issue draft before submitting it."
  ].join("\n");
}

export function generateDiagnosticsIssueUrl(report: string, options: { title?: string } = {}) {
  const url = new URL(diagnosticsIssueUrl);
  // Keep this as a browser draft so users can review the local report before sharing it.
  const body = [
    "## What happened?",
    "",
    "<!-- Describe the issue, what you expected, and steps to reproduce. -->",
    "",
    report
  ].join("\n");

  url.searchParams.set("title", options.title ?? "Diagnostics report");
  url.searchParams.set("body", body);

  return url.toString();
}

function normalizeCrashError(error: unknown) {
  if (error instanceof Error) {
    return {
      message: error.message.trim() || "Unknown error",
      name: error.name.trim() || "Error"
    };
  }

  if (typeof error === "string") {
    return {
      message: error.trim() || "Unknown error",
      name: "Error"
    };
  }

  return {
    message: "Unknown error",
    name: "Error"
  };
}

function line(label: string, value: boolean | number | string) {
  return `- ${label}: ${String(value)}`;
}

function minuteBucket(minutes: number) {
  if (!Number.isFinite(minutes) || minutes <= 0) return "disabled";
  if (minutes <= 1) return "1m";
  if (minutes <= 5) return "1-5m";
  if (minutes <= 15) return "5-15m";
  if (minutes <= 60) return "15-60m";
  if (minutes <= 240) return "1-4h";

  return ">4h";
}
