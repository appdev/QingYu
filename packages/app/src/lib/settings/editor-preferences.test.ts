import { defaultMarkdownShortcuts } from "@markra/editor";
import { createSettingsStoreHarness, resetSettingsStoreRuntime, setupSettingsStoreHarness } from "../../test/settings-store";
import {
  defaultEditorPreferences,
  getStoredEditorPreferences,
  normalizeEditorPreferences,
  reorderTitlebarActions,
  saveStoredEditorPreferences
} from "./app-settings";

const settingsStore = createSettingsStoreHarness();
const { loadStore: mockedLoadStore, store } = settingsStore;

describe("editor preferences", () => {
  beforeEach(() => {
    setupSettingsStoreHarness(settingsStore);
  });

  afterEach(() => {
    resetSettingsStoreRuntime();
  });

  it("retains only the local filename policy from legacy image settings", () => {
    const legacyCopySettingKey = ["copyExternalFiles", "ToStorage"].join("");
    const normalized = normalizeEditorPreferences({
      [legacyCopySettingKey]: false,
      imageUpload: {
        fileNamePattern: "{name}-{timestamp}",
        picgo: { secret: "", serverUrl: "https://example.test" },
        provider: "s3",
        s3: { accessKeyId: "" },
        webdav: { password: "" }
      }
    });

    expect(Object.keys(normalized.imageUpload)).toEqual(["fileNamePattern"]);
    expect(normalized.imageUpload.fileNamePattern).toBe("{name}-{timestamp}");
    expect(Object.keys(normalized)).not.toContain(legacyCopySettingKey);
  });

  it("loads default editor preferences", async () => {
    store.get.mockResolvedValue(undefined);

    await expect(getStoredEditorPreferences()).resolves.toEqual(defaultEditorPreferences);

    expect(store.get).toHaveBeenCalledWith("editorPreferences");
  });

  it("normalizes partial editor preferences from older settings files", async () => {
    store.get.mockResolvedValue({
      autoUpdateEnabled: true,
      autoSaveEnabled: false,
      autoSaveIntervalMinutes: 30,
      bodyFontSize: 99,
      clipboardImageFolder: "media/screenshots",
      contentWidth: "page",
      imageUpload: {
        fileNamePattern: "web-{name}-{timestamp}"
      },
      lineHeight: 2,
      markdownShortcuts: {
        bold: "Mod+Alt+B",
        italic: "Mod+S",
        quote: "Shift+B"
      },
      markdownTemplates: [
        {
          content: "# legacy content is migrated to the template file layer",
          fileName: " standup.md ",
          id: " standup ",
          name: " Standup ",
          suggestedName: " {{date}} standup "
        },
        {
          fileName: "../unsafe.md",
          id: "",
          name: "",
          suggestedName: ""
        }
      ],
      restoreWorkspaceOnStartup: false,
      sidebarLayoutMode: "stacked",
      showWordCount: false
    });

    await expect(getStoredEditorPreferences()).resolves.toEqual({
      autoRevealActiveFile: false,
      autoSaveEnabled: false,
      autoSaveIntervalMinutes: 30,
      autoUpdateEnabled: true,
      bodyFontSize: 16,
      clipboardImageFolder: "media/screenshots",
      contentWidth: "default",
      contentWidthPx: null,
      documentLinksOpen: true,
      documentLinksVisible: false,
      editorFontFamily: { family: null, source: "theme" },
      imageUpload: {
        fileNamePattern: "web-{name}-{timestamp}"
      },
      extendedSyntax: {
        githubAlerts: true,
        highlight: true
      },
      lineHeight: 1.65,
      markdownShortcuts: {
        ...defaultMarkdownShortcuts,
        bold: "Mod+Alt+B"
      },
      markdownTemplates: [
        {
          fileName: "standup.md",
          id: "standup",
          name: "Standup",
          suggestedName: "{{date}} standup"
        }
      ],
      paragraphSpacingPx: 8,
      restoreWorkspaceOnStartup: false,
      sidebarLayoutMode: "stacked",
      showDocumentTabs: true,
      splitVisualPanePercent: 50,
      tableColumnWidthMode: "auto",
      titlebarActions: [
        { id: "viewMode", visible: true },
        { id: "sourceMode", visible: true },
        { id: "history", visible: true },
        { id: "save", visible: true },
        { id: "theme", visible: true }
      ],
      viewMode: "daily",
      viewModeCustomizations: {
        documentLinks: "visible",
        documentTabs: "visible",
        fileList: "visible",
        fileTree: "visible",
        fileTreeButton: "visible",
        openButton: "visible",
        outline: "visible",
        quickCreateButton: "visible",
        recentFolders: "visible",
        sidebarLayout: "visible",
        statusBar: "visible",
        titlebarActions: "visible",
        viewModeToggle: "visible",
        wordCount: "visible"
      },
      showLineNumbers: false,
      showWordCount: false,
      wrapCodeBlocks: true
    });
  });

  it("normalizes custom paragraph spacing in pixels", () => {
    expect((normalizeEditorPreferences({ paragraphSpacingPx: 12 }) as Record<string, unknown>).paragraphSpacingPx).toBe(12);
    expect((normalizeEditorPreferences({ paragraphSpacingPx: 0 }) as Record<string, unknown>).paragraphSpacingPx).toBe(0);
    expect((normalizeEditorPreferences({ paragraphSpacingPx: -8 }) as Record<string, unknown>).paragraphSpacingPx).toBe(0);
    expect((normalizeEditorPreferences({ paragraphSpacingPx: 120 }) as Record<string, unknown>).paragraphSpacingPx).toBe(32);
    expect((normalizeEditorPreferences({ paragraphSpacingPx: "loose" }) as Record<string, unknown>).paragraphSpacingPx).toBe(8);
  });

  it("normalizes view mode preferences", () => {
    expect(normalizeEditorPreferences({}).viewMode).toBe("daily");
    expect(normalizeEditorPreferences({ viewMode: "focus" }).viewMode).toBe("focus");
    expect(normalizeEditorPreferences({ viewMode: "zen" }).viewMode).toBe("daily");
    expect(normalizeEditorPreferences({
      viewMode: "custom",
      viewModeCustomizations: {
        documentLinks: "hidden",
        documentTabs: "visible",
        fileTree: "hidden",
        fileTreeButton: "hidden",
        openButton: "hidden",
        quickCreateButton: "hidden",
        sidebarLayout: "hidden",
        statusBar: "no",
        titlebarActions: "hidden",
        viewModeToggle: "hidden",
        wordCount: "hidden"
      }
    }).viewModeCustomizations).toEqual({
      documentLinks: "hidden",
      documentTabs: "visible",
      fileList: "visible",
      fileTree: "hidden",
      fileTreeButton: "hidden",
      openButton: "hidden",
      outline: "visible",
      quickCreateButton: "hidden",
      recentFolders: "visible",
      sidebarLayout: "hidden",
      statusBar: "visible",
      titlebarActions: "hidden",
      viewModeToggle: "hidden",
      wordCount: "hidden"
    });
  });

  it("normalizes titlebar action ordering and visibility", () => {
    expect(normalizeEditorPreferences({
      titlebarActions: [
        { id: "save", visible: false },
        { id: "theme", visible: true },
        { id: "save", visible: true },
        { id: "splitMode", visible: true },
        { id: "unknown", visible: true },
        { id: "open", visible: true }
      ]
    }).titlebarActions).toEqual([
      { id: "save", visible: false },
      { id: "theme", visible: true },
      { id: "viewMode", visible: true },
      { id: "sourceMode", visible: true },
      { id: "history", visible: true }
    ]);
  });

  it("normalizes the automatic update preference", () => {
    expect(defaultEditorPreferences.autoUpdateEnabled).toBe(true);
    expect(normalizeEditorPreferences({}).autoUpdateEnabled).toBe(true);
    expect(normalizeEditorPreferences({ autoUpdateEnabled: true }).autoUpdateEnabled).toBe(true);
    expect(normalizeEditorPreferences({ autoUpdateEnabled: false }).autoUpdateEnabled).toBe(false);
    expect(normalizeEditorPreferences({ autoUpdateEnabled: "no" }).autoUpdateEnabled).toBe(true);
  });

   it("normalizes the automatic save preferences", () => {
    expect(normalizeEditorPreferences({}).autoSaveEnabled).toBe(true);
    expect(normalizeEditorPreferences({ autoSaveEnabled: false }).autoSaveEnabled).toBe(false);
    expect(normalizeEditorPreferences({ autoSaveEnabled: "no" }).autoSaveEnabled).toBe(true);
    expect(normalizeEditorPreferences({ autoSaveIntervalMinutes: 30 }).autoSaveIntervalMinutes).toBe(30);
    expect(normalizeEditorPreferences({ autoSaveIntervalMinutes: 0 }).autoSaveIntervalMinutes).toBe(1);
    expect(normalizeEditorPreferences({ autoSaveIntervalMinutes: 240 }).autoSaveIntervalMinutes).toBe(120);
    expect(normalizeEditorPreferences({ autoSaveIntervalMinutes: "often" }).autoSaveIntervalMinutes).toBe(10);
  });

  it("normalizes the editor font family preference", () => {
    expect(normalizeEditorPreferences({}).editorFontFamily).toEqual({ family: null, source: "theme" });
    expect(normalizeEditorPreferences({
      editorFontFamily: {
        family: "Example Serif",
        source: "system"
      }
    }).editorFontFamily).toEqual({ family: "Example Serif", source: "system" });
    expect(normalizeEditorPreferences({
      editorFontFamily: {
        family: "",
        source: "system"
      }
    }).editorFontFamily).toEqual({ family: null, source: "theme" });
    expect(normalizeEditorPreferences({ editorFontFamily: "Legacy Sans" }).editorFontFamily).toEqual({
      family: "Legacy Sans",
      source: "system"
    });
  });

  it("normalizes the code block line wrapping preference", () => {
    expect(normalizeEditorPreferences({}).wrapCodeBlocks).toBe(true);
    expect(normalizeEditorPreferences({ wrapCodeBlocks: false }).wrapCodeBlocks).toBe(false);
    expect(normalizeEditorPreferences({ wrapCodeBlocks: "no" }).wrapCodeBlocks).toBe(true);
  });

    it("normalizes extended syntax preferences", () => {
    expect(normalizeEditorPreferences({}).extendedSyntax).toEqual({
      githubAlerts: true,
      highlight: true
    });
    expect(normalizeEditorPreferences({
      extendedSyntax: {
        githubAlerts: false,
        highlight: false
      }
    }).extendedSyntax).toEqual({
      githubAlerts: false,
      highlight: false
    });
    expect(normalizeEditorPreferences({
      extendedSyntax: {
        githubAlerts: "maybe",
        highlight: "nope"
      }
    }).extendedSyntax).toEqual({
      githubAlerts: true,
      highlight: true
    });
    expect(normalizeEditorPreferences({
      extendedSyntax: null
    }).extendedSyntax).toEqual({
      githubAlerts: true,
      highlight: true
    });
  });

   it("moves titlebar actions to the target slot in both directions", () => {
    const actions = [
      { id: "viewMode", visible: true },
      { id: "sourceMode", visible: true },
      { id: "history", visible: true },
      { id: "save", visible: true },
      { id: "theme", visible: true }
    ] as const;

    expect(reorderTitlebarActions(actions, "save", "sourceMode")).toEqual([
      { id: "viewMode", visible: true },
      { id: "save", visible: true },
      { id: "sourceMode", visible: true },
      { id: "history", visible: true },
      { id: "theme", visible: true }
    ]);
  });

  it("normalizes custom markdown shortcuts while keeping unsafe chords at their defaults", () => {
    expect(normalizeEditorPreferences({
      markdownShortcuts: {
        bold: "mod+alt+b",
        inlineCode: "Mod+Shift+E",
        italic: "Mod+S",
        quote: "Alt+Q",
        strikethrough: "Mod+Shift+X"
      }
    }).markdownShortcuts).toEqual({
      ...defaultMarkdownShortcuts,
      bold: "Mod+Alt+B",
      strikethrough: "Mod+Shift+X"
    });
  });

  it("normalizes custom editor content width pixels", () => {
    expect(normalizeEditorPreferences({ contentWidthPx: 1120 }).contentWidthPx).toBe(1120);
    expect(normalizeEditorPreferences({ contentWidthPx: 320 }).contentWidthPx).toBe(640);
    expect(normalizeEditorPreferences({ contentWidthPx: 2000 }).contentWidthPx).toBe(1280);
    expect(normalizeEditorPreferences({ contentWidthPx: "wide" }).contentWidthPx).toBeNull();
  });

  it("normalizes split pane visual width percentages", () => {
    expect(normalizeEditorPreferences({ splitVisualPanePercent: 64 }).splitVisualPanePercent).toBe(64);
    expect(normalizeEditorPreferences({ splitVisualPanePercent: 10 }).splitVisualPanePercent).toBe(25);
    expect(normalizeEditorPreferences({ splitVisualPanePercent: 90 }).splitVisualPanePercent).toBe(75);
    expect(normalizeEditorPreferences({ splitVisualPanePercent: "wide" }).splitVisualPanePercent).toBe(50);
  });

  it("normalizes the sidebar layout mode", () => {
    expect(normalizeEditorPreferences({}).sidebarLayoutMode).toBe("stacked");
    expect(normalizeEditorPreferences({ sidebarLayoutMode: "tabs" }).sidebarLayoutMode).toBe("tabs");
    expect(normalizeEditorPreferences({ sidebarLayoutMode: "stacked" }).sidebarLayoutMode).toBe("stacked");
    expect(normalizeEditorPreferences({ sidebarLayoutMode: "paged" }).sidebarLayoutMode).toBe("stacked");
  });

   it("migrates previous app shortcut defaults to the current defaults", () => {
    expect(normalizeEditorPreferences({
      markdownShortcuts: {
        toggleSourceMode: "Mod+Alt+V"
      }
    }).markdownShortcuts).toEqual(defaultMarkdownShortcuts);
  });

  it("keeps markdown shortcut mappings unique after normalization", () => {
    expect(normalizeEditorPreferences({
      markdownShortcuts: {
        bold: "Mod+I"
      }
    }).markdownShortcuts).toEqual(defaultMarkdownShortcuts);

    expect(normalizeEditorPreferences({
      markdownShortcuts: {
        bold: "Mod+I",
        italic: "Mod+B"
      }
    }).markdownShortcuts).toEqual({
      ...defaultMarkdownShortcuts,
      bold: "Mod+I",
      italic: "Mod+B"
    });
  });

  it("falls back to the default filename when persisted image settings are unsafe", () => {
    expect(normalizeEditorPreferences({
      imageUpload: {
        fileNamePattern: "../bad"
      }
    }).imageUpload).toEqual({ fileNamePattern: "pasted-image-{timestamp}" });
  });

  it("falls back to the default clipboard image folder when the stored folder is unsafe", async () => {
    store.get.mockResolvedValue({
      clipboardImageFolder: "../outside"
    });

    await expect(getStoredEditorPreferences()).resolves.toEqual(defaultEditorPreferences);
  });

  it("persists editor preferences", async () => {
    await saveStoredEditorPreferences({
      autoRevealActiveFile: true,
      autoSaveEnabled: true,
      autoSaveIntervalMinutes: 10,
      autoUpdateEnabled: true,
      bodyFontSize: 18,
      clipboardImageFolder: "images",
      contentWidth: "wide",
      contentWidthPx: 1120,
      documentLinksOpen: true,
      documentLinksVisible: false,
      editorFontFamily: { family: "Example Serif", source: "system" },
      extendedSyntax: {
        githubAlerts: false,
        highlight: false
      },
      imageUpload: {
        fileNamePattern: "{name}-{timestamp}"
      },
      lineHeight: 1.8,
      markdownShortcuts: {
        ...defaultMarkdownShortcuts,
        bold: "Mod+Alt+B"
      },
      markdownTemplates: [
        {
          fileName: "weekly-review.md",
          id: "weekly-review",
          name: "Weekly review",
          suggestedName: "{{date}} weekly"
        }
      ],
      paragraphSpacingPx: 8,
      restoreWorkspaceOnStartup: false,
      sidebarLayoutMode: "tabs",
      showDocumentTabs: false,
      splitVisualPanePercent: 64,
      tableColumnWidthMode: "even",
      titlebarActions: [
        { id: "theme", visible: true },
        { id: "save", visible: false },
        { id: "sourceMode", visible: true },
        { id: "history", visible: true },
        { id: "viewMode", visible: true }
      ],
      viewMode: "custom",
      viewModeCustomizations: {
        documentLinks: "visible",
        documentTabs: "hidden",
        fileList: "visible",
        fileTree: "visible",
        fileTreeButton: "visible",
        openButton: "visible",
        outline: "visible",
        quickCreateButton: "visible",
        recentFolders: "visible",
        sidebarLayout: "visible",
        statusBar: "hidden",
        titlebarActions: "visible",
        viewModeToggle: "visible",
        wordCount: "visible"
      },
      showLineNumbers: false,
      showWordCount: false,
      wrapCodeBlocks: false
    });

    expect(store.set).toHaveBeenCalledWith("editorPreferences", {
      autoRevealActiveFile: true,
      autoSaveEnabled: true,
      autoSaveIntervalMinutes: 10,
      autoUpdateEnabled: true,
      bodyFontSize: 18,
      clipboardImageFolder: "images",
      contentWidth: "wide",
      contentWidthPx: 1120,
      documentLinksOpen: true,
      documentLinksVisible: false,
      editorFontFamily: { family: "Example Serif", source: "system" },
      extendedSyntax: {
        githubAlerts: false,
        highlight: false
      },
      imageUpload: {
        fileNamePattern: "{name}-{timestamp}"
      },
      lineHeight: 1.8,
      markdownShortcuts: {
        ...defaultMarkdownShortcuts,
        bold: "Mod+Alt+B"
      },
      markdownTemplates: [
        {
          fileName: "weekly-review.md",
          id: "weekly-review",
          name: "Weekly review",
          suggestedName: "{{date}} weekly"
        }
      ],
      paragraphSpacingPx: 8,
      restoreWorkspaceOnStartup: false,
      sidebarLayoutMode: "tabs",
      showDocumentTabs: false,
      splitVisualPanePercent: 64,
      tableColumnWidthMode: "even",
      titlebarActions: [
        { id: "theme", visible: true },
        { id: "save", visible: false },
        { id: "sourceMode", visible: true },
        { id: "history", visible: true },
        { id: "viewMode", visible: true }
      ],
      viewMode: "custom",
      viewModeCustomizations: {
        documentLinks: "visible",
        documentTabs: "hidden",
        fileList: "visible",
        fileTree: "visible",
        fileTreeButton: "visible",
        openButton: "visible",
        outline: "visible",
        quickCreateButton: "visible",
        recentFolders: "visible",
        sidebarLayout: "visible",
        statusBar: "hidden",
        titlebarActions: "visible",
        viewModeToggle: "visible",
        wordCount: "visible"
      },
      showLineNumbers: false,
      showWordCount: false,
      wrapCodeBlocks: false
    });
    expect(store.save).toHaveBeenCalledTimes(1);
  });
});
