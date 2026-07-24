import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { mobileRuntime } from "./mobile";
import * as fileConfirm from "./tauri/file/confirm";
import * as mobileFiles from "./tauri/file/mobile";
import * as files from "./tauri/file/shared";
import * as mobileBack from "./tauri/mobile-back";
import * as themes from "./tauri/themes/shared";
import * as mcpPolicy from "./tauri/mcp-policy";
import * as managedWorkspace from "./tauri/managed-workspace";
import * as settings from "./tauri/settings";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn(),
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn()
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn()
}));

vi.mock("@tauri-apps/plugin-fs", () => ({
  readFile: vi.fn()
}));

vi.mock("@tauri-apps/plugin-log", () => ({
  error: vi.fn(),
  info: vi.fn(),
  warn: vi.fn()
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn()
}));

const mockedConfirm = vi.mocked(confirm);
const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);
const rootPath = "/mobile/workspace";
const notePath = `${rootPath}/note.md`;

describe("mobile runtime import boundary", () => {
  it("imports only mobile-safe file, log, application sync, settings, MCP, and theme adapters", () => {
    const source = readFileSync(resolve(process.cwd(), "src/runtime/mobile.ts"), "utf8");

    expect(source).toContain('from "./tauri/file/shared"');
    expect(source).toContain('from "./tauri/file/mobile"');
    expect(source).toContain('from "./tauri/file/confirm"');
    expect(source).toContain('from "./tauri/logs/shared"');
    expect(source).toContain('from "./tauri/sync-config/shared"');
    expect(source).not.toMatch(/from "\.\/tauri\/(?:file|logs|sync-config)"/u);
    expect(source).toContain('from "./tauri/mcp-policy"');
    expect(source).not.toMatch(/from "\.\/tauri\/mcp"/u);
    expect(source).toContain('from "./tauri/settings"');
    expect(source).toContain('from "./tauri/themes/shared"');
    expect(source).not.toMatch(/from "\.\/tauri\/themes"/u);
    expect(mobileRuntime.mcp.policyAvailable).toBe(true);
    expect(mobileRuntime.mcp.localServiceAvailable).toBe(false);
    expect(mobileRuntime.mcp.getSettings).toBe(mcpPolicy.getNativeMcpPolicySettings);
    expect(mobileRuntime.mcp.updateSettings).toBe(mcpPolicy.updateNativeMcpPolicySettings);
    expect(mobileRuntime.settings.readGroup).toBe(settings.readNativeAppSettingsGroup);
    expect(mobileRuntime.settings.replacePortable).toBe(settings.replaceNativePortableAppSettings);
    expect(mobileRuntime.settings.writeGroup).toBe(settings.writeNativeAppSettingsGroup);
  });

  it("reads and revision-writes MCP policy through mobile native commands", async () => {
    const config = { enabled: false } as Parameters<typeof mobileRuntime.mcp.updateSettings>[0]["config"];
    const enabledConfig = { ...config, enabled: true };
    mockedInvoke
      .mockResolvedValueOnce({ config, revision: "revision-1" })
      .mockResolvedValueOnce({ config: enabledConfig, revision: "revision-2" });

    await expect(mobileRuntime.mcp.getSettings()).resolves.toMatchObject({
      endpoint: null,
      health: { state: "stopped" },
      revision: "revision-1",
      workspace: null
    });
    await expect(mobileRuntime.mcp.updateSettings({
      config: enabledConfig,
      expectedRevision: "revision-1"
    })).resolves.toMatchObject({ revision: "revision-2" });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "get_mcp_policy");
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "update_mcp_policy", {
      input: {
        config: enabledConfig,
        expectedRevision: "revision-1"
      }
    });
  });
});

describe("mobile runtime core file surface", () => {
  beforeEach(() => {
    mockedConfirm.mockReset();
    mockedInvoke.mockReset();
    mockedListen.mockReset();
  });

  it("binds every approved core method to the mobile-safe adapters", () => {
    expect(mobileRuntime.files).toMatchObject({
      confirmMarkdownFileDelete: fileConfirm.confirmNativeMarkdownFileDelete,
      confirmUnsavedMarkdownDocumentDiscard: fileConfirm.confirmNativeUnsavedMarkdownDocumentDiscard,
      createMarkdownTreeFile: files.createNativeMarkdownTreeFile,
      createMarkdownTreeFolder: files.createNativeMarkdownTreeFolder,
      deleteMarkdownTreeFile: files.deleteNativeMarkdownTreeFile,
      listMarkdownFileHistory: files.listNativeMarkdownFileHistory,
      listMarkdownFilesForPath: files.listNativeMarkdownFilesForPath,
      loadMarkdownFilesForPath: files.loadNativeMarkdownFilesForPath,
      moveMarkdownTreeFile: files.moveNativeMarkdownTreeFile,
      openLocalImages: mobileFiles.openMobileLocalImages,
      readMarkdownFile: files.readNativeMarkdownFile,
      readMarkdownFileHistory: files.readNativeMarkdownFileHistory,
      renameMarkdownTreeFile: files.renameNativeMarkdownTreeFile,
      saveMarkdownFile: files.saveNativeMarkdownFileInPlace,
      saveClipboardImage: mobileFiles.saveMobileClipboardImage,
      searchMarkdownFiles: files.searchNativeMarkdownFilesForPath,
      watchMarkdownFile: files.watchNativeMarkdownFile,
      watchMarkdownTree: files.watchNativeMarkdownTree
    });
    expect(mobileRuntime.navigation.subscribeToSystemBack).toBe(mobileBack.subscribeToMobileSystemBack);
    const workspace = mobileRuntime.workspace as typeof mobileRuntime.workspace & {
      listManagedNotebookNames?: () => Promise<string[]>;
    };
    const adapter = managedWorkspace as typeof managedWorkspace & {
      listNativeManagedWorkspaceNames?: () => Promise<string[]>;
    };
    expect(workspace.listManagedNotebookNames).toBe(adapter.listNativeManagedWorkspaceNames);
    expect(workspace.listManagedNotebookNames).toEqual(expect.any(Function));
  });

  it("enables URI image import without exposing arbitrary local file import", async () => {
    expect(mobileRuntime.features.imageImport).toBe(true);
    expect(mobileRuntime.files.openLocalImages).toBe(mobileFiles.openMobileLocalImages);
    expect(mobileRuntime.files.saveClipboardImage).toBe(mobileFiles.saveMobileClipboardImage);
    await expect(mobileRuntime.files.openLocalFiles()).resolves.toEqual([]);
    await expect(mobileRuntime.files.importLocalFile({
      copyToStorage: true,
      documentPath: notePath,
      file: { name: "attachment.pdf", path: "content://document/42" },
      folder: "assets",
      projectRootPath: rootPath
    })).rejects.toThrow(/unavailable/i);
  });

  it("uses the native confirmation adapter for delete and discard", async () => {
    mockedConfirm.mockResolvedValue(true);

    await expect(mobileRuntime.files.confirmMarkdownFileDelete("note.md", {
      cancelLabel: "Cancel",
      message: "Delete note?",
      okLabel: "Delete"
    })).resolves.toBe(true);
    await expect(mobileRuntime.files.confirmUnsavedMarkdownDocumentDiscard("note.md", {
      cancelLabel: "Cancel",
      message: "Discard changes?",
      okLabel: "Discard"
    })).resolves.toBe(true);

    expect(mockedConfirm.mock.calls).toEqual([
      ["Delete note?", {
        cancelLabel: "Cancel",
        kind: "warning",
        okLabel: "Delete",
        title: "note.md"
      }],
      ["Discard changes?", {
        cancelLabel: "Cancel",
        kind: "warning",
        okLabel: "Discard",
        title: "note.md"
      }]
    ]);
  });

  it("invokes the complete mobile-safe tree mutation surface", async () => {
    mockedInvoke
      .mockResolvedValueOnce({ path: notePath, relativePath: "note.md" })
      .mockResolvedValueOnce({ kind: "folder", path: `${rootPath}/docs`, relativePath: "docs" })
      .mockResolvedValueOnce({ path: `${rootPath}/docs/note.md`, relativePath: "docs/note.md" })
      .mockResolvedValueOnce({ path: `${rootPath}/renamed.md`, relativePath: "renamed.md" })
      .mockResolvedValueOnce(undefined);

    await mobileRuntime.files.createMarkdownTreeFile(rootPath, "note.md", { contents: "# Note" });
    await mobileRuntime.files.createMarkdownTreeFolder(rootPath, "docs");
    await mobileRuntime.files.moveMarkdownTreeFile(rootPath, notePath, `${rootPath}/docs`);
    await mobileRuntime.files.renameMarkdownTreeFile(rootPath, notePath, "renamed.md");
    await mobileRuntime.files.deleteMarkdownTreeFile(rootPath, notePath);

    expect(mockedInvoke.mock.calls).toEqual([
      ["create_markdown_tree_file", {
        contents: "# Note",
        fileName: "note.md",
        parentPath: null,
        rootPath
      }],
      ["create_markdown_tree_folder", {
        folderName: "docs",
        parentPath: null,
        rootPath
      }],
      ["move_markdown_tree_file", {
        path: notePath,
        rootPath,
        targetParentPath: `${rootPath}/docs`
      }],
      ["rename_markdown_tree_file", {
        fileName: "renamed.md",
        path: notePath,
        rootPath
      }],
      ["delete_markdown_tree_file", {
        path: notePath,
        rootPath
      }]
    ]);
  });

  it("lists, reads, saves, searches, and reads history through native commands", async () => {
    mockedInvoke
      .mockResolvedValueOnce([{ path: notePath, relativePath: "note.md", sizeBytes: 6 }])
      .mockResolvedValueOnce({ path: notePath, contents: "# Note", sizeBytes: 6 })
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([{ id: "history-1", createdAt: 1, sizeBytes: 4 }])
      .mockResolvedValueOnce({ id: "history-1", contents: "Old" })
      .mockResolvedValueOnce({
        results: [],
        searchedFileCount: 1,
        truncated: false,
        unreadableFileCount: 0
      });

    await expect(mobileRuntime.files.listMarkdownFilesForPath(rootPath)).resolves.toEqual([
      { path: notePath, name: "note.md", relativePath: "note.md", sizeBytes: 6 }
    ]);
    await expect(mobileRuntime.files.readMarkdownFile(notePath)).resolves.toEqual({
      path: notePath,
      name: "note.md",
      content: "# Note",
      sizeBytes: 6
    });
    await expect(mobileRuntime.files.saveMarkdownFile({
      path: notePath,
      suggestedName: "note.md",
      contents: "# Updated"
    })).resolves.toEqual({ path: notePath, name: "note.md" });
    await expect(mobileRuntime.files.listMarkdownFileHistory(notePath)).resolves.toHaveLength(1);
    await expect(mobileRuntime.files.readMarkdownFileHistory(notePath, "history-1")).resolves.toEqual({
      id: "history-1",
      contents: "Old"
    });
    await expect(mobileRuntime.files.searchMarkdownFiles?.({
      path: rootPath,
      query: "needle"
    })).resolves.toMatchObject({ searchedFileCount: 1 });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "list_markdown_files_for_path", { path: rootPath });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "read_markdown_file", { path: notePath });
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, "write_markdown_file", {
      contents: "# Updated",
      path: notePath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(4, "list_markdown_file_history", { path: notePath });
    expect(mockedInvoke).toHaveBeenNthCalledWith(5, "read_markdown_file_history", {
      id: "history-1",
      path: notePath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(6, "search_markdown_files_for_path", expect.objectContaining({
      path: rootPath,
      query: "needle"
    }));
  });

  it("loads a tree through the native incremental-load event", async () => {
    const unlisten = vi.fn();
    let emitTreeLoad: (payload: unknown) => unknown = () => undefined;
    mockedListen.mockImplementation(async (event, handler) => {
      if (event === "markra://markdown-tree-load") {
        emitTreeLoad = (payload) => handler({ payload } as never);
      }
      return unlisten;
    });
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === "load_markdown_files_for_path") {
        const requestId = (args as { requestId: string }).requestId;
        emitTreeLoad({
          done: true,
          files: [{ path: notePath, relativePath: "note.md" }],
          requestId
        });
      }
      return undefined;
    });

    await expect(mobileRuntime.files.loadMarkdownFilesForPath?.(rootPath)).resolves.toEqual([
      { path: notePath, name: "note.md", relativePath: "note.md" }
    ]);
    expect(mockedInvoke).toHaveBeenCalledWith("load_markdown_files_for_path", expect.objectContaining({
      path: rootPath,
      requestId: expect.any(String)
    }));
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("starts and stops native file and tree watchers", async () => {
    const unlisten = vi.fn();
    mockedListen.mockResolvedValue(unlisten);
    mockedInvoke.mockResolvedValue(undefined);

    const stopFile = await mobileRuntime.files.watchMarkdownFile(notePath, vi.fn());
    const stopTree = await mobileRuntime.files.watchMarkdownTree(rootPath, vi.fn());
    stopFile();
    stopTree();

    expect(mockedListen).toHaveBeenCalledWith("markra://file-changed", expect.any(Function));
    expect(mockedListen).toHaveBeenCalledWith("markra://tree-changed", expect.any(Function));
    expect(mockedInvoke).toHaveBeenCalledWith("watch_markdown_file", { path: notePath });
    expect(mockedInvoke).toHaveBeenCalledWith("watch_markdown_tree", { rootPath });
    expect(mockedInvoke).toHaveBeenCalledWith("unwatch_markdown_file", { path: notePath });
    expect(mockedInvoke).toHaveBeenCalledWith("unwatch_markdown_tree", { rootPath });
  });
});

describe("mobile runtime theme surface", () => {
  it("supports activation while retaining mobile capability limits", () => {
    expect(mobileRuntime.themes.capabilities).toEqual({
      canDelete: true,
      canImport: false,
      canOpenDirectory: false
    });
    expect(mobileRuntime.themes).toMatchObject({
      cancelActivation: themes.cancelNativeThemeActivation,
      commitActivation: themes.commitNativeThemeActivation,
      prepareActivation: themes.prepareNativeThemeActivation,
      releaseActivation: themes.releaseNativeThemeActivation
    });
    expect(mobileRuntime.themes).not.toHaveProperty("readCss");
  });
});
