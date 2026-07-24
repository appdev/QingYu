import type { ReactNode } from "react";
import type { AppLanguage, MarkdownFormattingShortcutAction } from "@markra/shared";
import type { SelectionHeadingLevel } from "../../lib/selection-formatting";
import type { NativeMarkdownFolderFile } from "../../lib/tauri";
import type { useEditorController } from "../../hooks/useEditorController";
import type { useEditorPreferences } from "../../hooks/useEditorPreferences";
import type { useAppLanguage } from "../../hooks/useAppLanguage";
import type { useAppTheme } from "../../hooks/useAppTheme";
import type { useMarkdownDocument } from "../../hooks/useMarkdownDocument";
import type { useMarkdownFileTree } from "../../hooks/useMarkdownFileTree";
import type { CompactAutoSaveState } from "../../hooks/useCompactAutoSave";
import type { CompactSyncSettingsController } from "../../hooks/useCompactSyncSettings";
import type { SyncConfigDocument } from "../../lib/sync-config";
import type { AppMcpRuntime } from "../../runtime";
import type { CompactOverlayPage } from "../../hooks/useCompactNavigation";

type MarkdownDocumentController = ReturnType<typeof useMarkdownDocument>;
type MarkdownFileTreeController = ReturnType<typeof useMarkdownFileTree>;
type EditorController = ReturnType<typeof useEditorController>;
type EditorPreferencesController = ReturnType<typeof useEditorPreferences>;
type AppLanguageController = ReturnType<typeof useAppLanguage>;
type AppThemeController = ReturnType<typeof useAppTheme>;

export type CompactDocumentController = Pick<
  MarkdownDocumentController,
  "document" | "saveCurrentDocument"
> & {
  createBlankDocument: (fileName: string) => Promise<boolean>;
};

export type CompactEditorController = {
  getSelectionFormattingState: EditorController["getSelectionFormattingState"];
  host: ReactNode;
  importLocalImages: () => Promise<unknown> | unknown;
  insertMarkdownImage: () => unknown;
  insertMarkdownLink: () => unknown;
  readOnly: boolean;
  runEditorShortcut: EditorController["runEditorShortcut"];
  runFormattingAction: (action: MarkdownFormattingShortcutAction) => unknown;
  setSelectionHeadingLevel: (level: SelectionHeadingLevel) => unknown;
  toggleTaskList: () => unknown;
};

export type CompactFilesController = Pick<
  MarkdownFileTreeController,
  | "createFile"
  | "createFolder"
  | "files"
  | "sourcePath"
> & {
  deleteFile: (file: NativeMarkdownFolderFile) => Promise<unknown> | unknown;
  moveFile: (file: NativeMarkdownFolderFile, targetParentPath: string | null) => Promise<boolean> | boolean;
  openFile: (file: NativeMarkdownFolderFile) => Promise<unknown> | unknown;
  openMarkdownFolder: () => Promise<unknown> | unknown;
  renameFile: (file: NativeMarkdownFolderFile, fileName: string) => Promise<unknown> | unknown;
};

export type CompactPreferencesController = Pick<
  EditorPreferencesController,
  "loading" | "preferences" | "updatePreferences"
>;

export type CompactWorkspaceController = {
  openNotebookManager?: () => unknown;
  primaryRoot: string | null;
  syncConfigDocument: SyncConfigDocument | null;
};

export type CompactSaveState = CompactAutoSaveState;

export type CompactAppActions = {
  openDocumentHistory: () => Promise<unknown> | unknown;
  openDocumentSearch: () => Promise<unknown> | unknown;
  runApplicationSyncNow: () => Promise<unknown> | unknown;
  saveDocument: () => Promise<unknown> | unknown;
};

export type CompactAppController = {
  actions: CompactAppActions;
  appearance: Pick<AppThemeController,
    | "activeTheme"
    | "appearanceMode"
    | "catalog"
    | "darkTheme"
    | "lightTheme"
    | "selectAppearanceMode"
    | "selectTheme"
    | "themeError"
  >;
  capabilities: {
    imageImport: boolean;
    openLocalAttachments: boolean;
    applicationSync: boolean;
    mcpPolicy: boolean;
    systemFonts: boolean;
    trueMobile: boolean;
  };
  document: CompactDocumentController;
  editor: CompactEditorController;
  files: CompactFilesController;
  language: AppLanguage;
  mcp: AppMcpRuntime;
  navigationRequest?: {
    id: number;
    page: CompactOverlayPage;
    retainUntilEditor: boolean;
  } | null;
  preferences: CompactPreferencesController;
  workspace: CompactWorkspaceController;
  saveState: CompactSaveState;
  selectLanguage: AppLanguageController["selectLanguage"];
  sync: CompactSyncSettingsController;
};

export type CompactFileBrowserController = Pick<CompactAppController, "files" | "language" | "saveState">;
