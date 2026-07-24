import {
  defaultEditorPreferences,
  defaultExportSettings
} from "../settings/app-settings";
import {
  generateCrashDiagnosticsReport,
  generateDiagnosticsIssueUrl,
  generateDiagnosticsReport
} from "./diagnostics-report";

describe("generateDiagnosticsReport", () => {
  it("formats a local Markdown report from non-sensitive settings", () => {
    const report = generateDiagnosticsReport({
      appVersion: "9.9.9",
      editorPreferences: {
        ...defaultEditorPreferences,
        autoSaveIntervalMinutes: 10,
        imageUpload: { fileNamePattern: "{name}-{timestamp}" }
      },
      exportSettings: {
        ...defaultExportSettings,
        pandocArgs: "--resource-path=/Users/example/private-vault",
        pandocPath: "/Users/example/bin/pandoc",
        pdfAuthor: "Ada Private"
      },
      features: {
        applicationMenu: true,
        applicationShortcuts: true,
        export: true,
        fileDrop: true,
        imageImport: true,
        nativeWindowChrome: true,
        openLocalAttachments: true,
        pandoc: true,
        projectSync: true,
        resources: true,
        settingsWindow: true,
        systemFonts: true,
        updater: true
      },
      generatedAt: new Date("2030-01-02T03:04:05.000Z"),
      language: "zh-CN",
      osVersion: "15.5",
      platform: "macos"
    });

    expect(report).toContain("## QingYu Diagnostics");
    expect(report).toContain("- App version: 9.9.9");
    expect(report).toContain("- Platform: macos");
    expect(report).toContain("- OS version: 15.5");
    expect(report).toContain("- App language: zh-CN");
    expect(report).toContain("- Auto-save interval: 5-15m");
    expect(report).toContain("- Application menu enabled: true");
    expect(report).toContain("- Application shortcuts enabled: true");
    expect(report).toContain("- Export feature enabled: true");
    expect(report).toContain("- File drop enabled: true");
    expect(report).toContain("- Image import enabled: true");
    expect(report).toContain("- Native window chrome enabled: true");
    expect(report).toContain("- Local attachment opening enabled: true");
    expect(report).toContain("- Pandoc feature enabled: true");
    expect(report).toContain("- Primary notes sync enabled: true");
    expect(report).toContain("- Settings window enabled: true");
    expect(report).toContain("- System fonts enabled: true");
    expect(report).toContain("- Updater feature enabled: true");
    expect(report).not.toContain("Network proxy");
    expect(report).not.toContain("Bypass local addresses");
    expect(report).not.toContain("### Network");
    expect(report).not.toContain("Image storage provider");
    expect(report).not.toContain("S3 image upload feature");
    expect(report).not.toContain("Sync enabled");
    expect(report).not.toContain("Sync provider");

    for (const sensitiveValue of [
      "/Users/example",
      "Ada Private",
      "private-vault",
      "user:secret"
    ]) {
      expect(report).not.toContain(sensitiveValue);
    }
  });
});

describe("generateDiagnosticsIssueUrl", () => {
  it("builds a GitHub issue draft with the diagnostics report prefilled", () => {
    const report = [
      "## QingYu Diagnostics",
      "- App version: 9.9.9",
      "- Platform: macos"
    ].join("\n");
    const issueUrl = new URL(generateDiagnosticsIssueUrl(report));

    expect(issueUrl.origin).toBe("https://github.com");
    expect(issueUrl.pathname).toBe("/appdev/QingYu/issues/new");
    expect(issueUrl.searchParams.get("title")).toBe("Diagnostics report");
    expect(issueUrl.searchParams.get("body")).toContain("## What happened?");
    expect(issueUrl.searchParams.get("body")).toContain(report);
  });

  it("uses a custom issue title when provided", () => {
    const issueUrl = new URL(generateDiagnosticsIssueUrl("## QingYu Crash Report", { title: "Crash report" }));

    expect(issueUrl.searchParams.get("title")).toBe("Crash report");
  });
});

describe("generateCrashDiagnosticsReport", () => {
  it("formats a crash report without raw error stacks", () => {
    const error = new Error("Render exploded");
    error.stack = "Error: Render exploded\n    at /Users/example/private-project/src/App.tsx:1:1";
    const report = generateCrashDiagnosticsReport({
      appVersion: "9.9.9",
      componentStack: "\n    at BrokenPanel\n    at App",
      error,
      generatedAt: new Date("2030-01-02T03:04:05.000Z"),
      language: "zh-CN",
      osVersion: "15.5",
      platform: "macos"
    });

    expect(report).toContain("## QingYu Crash Report");
    expect(report).toContain("- Error name: Error");
    expect(report).toContain("- Error message: Render exploded");
    expect(report).toContain("- Component stack: available");
    expect(report).toContain("- Platform: macos");
    expect(report).toContain("- App language: zh-CN");
    expect(report).not.toContain("/Users/example");
    expect(report).not.toContain("private-project");
  });
});
