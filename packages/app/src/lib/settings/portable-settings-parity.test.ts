import { formatKeyboardShortcut, isAppLanguage } from "@markra/shared";
import golden from "./portable-settings.golden.json";
import {
  normalizeAppThemePreferences,
  normalizeCustomThemeCss,
  normalizeEditorPreferences
} from "./app-settings";
import { normalizePortableExportSettings } from "./export-settings";
import { normalizeFileIgnoreSettings } from "./file-ignore-settings";

function editorWith(patch: Record<string, unknown>) {
  return {
    ...structuredClone(golden.validStore.editorPreferences),
    ...patch
  };
}

describe("portable settings Rust/TypeScript schema parity", () => {
  it("keeps the shared complete settings fixture unchanged through TypeScript writers", () => {
    const store = golden.validStore;

    expect(normalizeAppThemePreferences({
      appearanceMode: store.appearanceMode,
      darkTheme: store.darkThemeId,
      lightTheme: store.lightThemeId
    })).toEqual({
      appearanceMode: store.appearanceMode,
      darkTheme: store.darkThemeId,
      lightTheme: store.lightThemeId
    });
    expect(normalizeCustomThemeCss(store.lightCustomThemeCss)).toBe(store.lightCustomThemeCss);
    expect(normalizeCustomThemeCss(store.darkCustomThemeCss)).toBe(store.darkCustomThemeCss);
    expect(isAppLanguage(store.language)).toBe(true);
    expect(normalizeEditorPreferences(store.editorPreferences)).toEqual(store.editorPreferences);
    expect(normalizeFileIgnoreSettings(store.fileIgnoreSettings)).toEqual(store.fileIgnoreSettings);
    expect(normalizePortableExportSettings(store.exportSettings)).toEqual(store.exportSettings);
    expect(store).not.toHaveProperty("mcp");
  });

  it("documents noncanonical nested values that TypeScript writers change", () => {
    const incompleteExtendedSyntax = editorWith({ extendedSyntax: { githubAlerts: true } });
    const reservedShortcut = editorWith({
      markdownShortcuts: {
        ...golden.validStore.editorPreferences.markdownShortcuts,
        bold: "Mod+S"
      }
    });
    const duplicateShortcut = editorWith({
      markdownShortcuts: {
        ...golden.validStore.editorPreferences.markdownShortcuts,
        bold: "Mod+I"
      }
    });
    const duplicateTemplateId = editorWith({
      markdownTemplates: [
        ...golden.validStore.editorPreferences.markdownTemplates,
        { fileName: "second.md", id: "daily", name: "Second", suggestedName: "" }
      ]
    });
    const duplicateTemplateFileName = editorWith({
      markdownTemplates: [
        ...golden.validStore.editorPreferences.markdownTemplates,
        { fileName: "DAILY.MD", id: "second", name: "Second", suggestedName: "" }
      ]
    });

    for (const candidate of [
      incompleteExtendedSyntax,
      reservedShortcut,
      duplicateShortcut,
      duplicateTemplateId,
      duplicateTemplateFileName,
      editorWith({ clipboardImageFolder: "assets\\screenshots" }),
      editorWith({ clipboardImageFolder: "assets//screenshots" }),
      editorWith({ clipboardImageFolder: "./assets" })
    ]) {
      expect(normalizeEditorPreferences(candidate)).not.toEqual(candidate);
    }
  });

  it("keeps template metadata beyond 500 UTF-16 code units because the writer has no such limit", () => {
    const longPart = "文".repeat(501);
    const preferences = editorWith({
      markdownTemplates: [{
        fileName: `${longPart}.md`,
        id: longPart,
        name: longPart,
        suggestedName: longPart
      }]
    });

    expect(normalizeEditorPreferences(preferences)).toEqual(preferences);
  });

  it.each(golden.boundaryCases.invalidClipboardImageFolders)(
    "rejects the shared clipboard image folder boundary %j",
    (folder) => {
      const preferences = editorWith({ clipboardImageFolder: folder });

      expect(normalizeEditorPreferences(preferences).clipboardImageFolder).toBe(
        golden.validStore.editorPreferences.clipboardImageFolder
      );
    }
  );

  it.each(golden.boundaryCases.invalidShortcutKeys)(
    "rejects the shared non-ASCII shortcut key boundary %s",
    (shortcut) => {
      expect(formatKeyboardShortcut(shortcut)).toBeNull();

      const preferences = editorWith({
        markdownShortcuts: {
          ...golden.validStore.editorPreferences.markdownShortcuts,
          bold: shortcut
        }
      });

      expect(normalizeEditorPreferences(preferences).markdownShortcuts.bold).toBe(
        golden.validStore.editorPreferences.markdownShortcuts.bold
      );
    }
  );
});
