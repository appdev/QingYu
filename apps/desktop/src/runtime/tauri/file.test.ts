import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import {
  confirmNativeMarkdownFileDelete,
  confirmNativeWorkspaceResourceTrash,
  confirmNativeUnsavedMarkdownDocumentDiscard,
  createNativeMarkdownTreeFile,
  createNativeMarkdownTreeFolder,
  deleteNativeMarkdownTemplateFile,
  deleteNativeMarkdownTreeFile,
  detectNativePandocPath,
  installNativeMarkdownFileDrop,
  listenNativeOpenedMarkdownPaths,
  listNativeMarkdownFileHistory,
  loadNativeMarkdownFilesForPath,
  listNativeMarkdownFilesForPath,
  moveNativeMarkdownTreeFile,
  importNativeLocalFile,
  openNativeLocalImages,
  openNativeLocalFiles,
  takeNativeOpenedMarkdownPaths,
  openNativeContainingFolder,
  openNativeMarkdownAttachment,
  openNativeMarkdownFolder,
  openNativeMarkdownFile,
  openNativeMarkdownFileInNewWindow,
  openNativeSettingsFile,
  readNativeMarkdownFileHistory,
  readNativeMarkdownFile,
  readNativeMarkdownTemplateFile,
  requestNativePrimaryNotebookSwitch,
  resolveNativeMarkdownFolder,
  resolveNativeMarkdownPath,
  resolveNativeWorkspaceResourceRoot,
  saveNativeClipboardAttachment,
  saveNativeClipboardImage,
  saveNativeHtmlFile,
  saveNativePandocFile,
  saveNativePdfFile,
  renameNativeMarkdownTreeFile,
  saveNativeSettingsFile,
  saveNativeMarkdownFile,
  searchNativeMarkdownFilesForPath,
  trashNativeWorkspaceResources,
  writeNativeMarkdownTemplateFile,
  watchNativeMarkdownFile,
  watchNativeMarkdownTree
} from "./file";
import { downloadNativeWebImage } from "./web-resource";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn()
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn()
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn()
}));

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);
const mockedGetCurrentWindow = vi.mocked(getCurrentWindow);
const mockedConfirm = vi.mocked(confirm);
const mockedOpen = vi.mocked(open);
const mockedSave = vi.mocked(save);

const mockReadmePath = "/mock-files/readme.md";
const mockDraftPath = "/mock-files/draft.md";
const mockFolderPath = "/mock-files/vault";
const mockUntitledPath = "/mock-files/Untitled.md";

describe("native file access", () => {
  const onDragDropEvent = vi.fn();

  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedListen.mockReset();
    mockedGetCurrentWindow.mockReset();
    mockedConfirm.mockReset();
    mockedOpen.mockReset();
    mockedSave.mockReset();
    onDragDropEvent.mockReset();
    mockedGetCurrentWindow.mockReturnValue({
      onDragDropEvent
    } as unknown as ReturnType<typeof getCurrentWindow>);
  });

  it("opens a markdown file through the native dialog and Tauri command", async () => {
    mockedOpen.mockResolvedValue(mockReadmePath);
    mockedInvoke.mockResolvedValue({
      path: mockReadmePath,
      contents: "# Native"
    });

    await expect(openNativeMarkdownFile()).resolves.toEqual({
      path: mockReadmePath,
      name: "readme.md",
      content: "# Native"
    });

    expect(mockedOpen).toHaveBeenCalledWith({
      multiple: false,
      fileAccessMode: "scoped",
      filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }]
    });
    expect(mockedInvoke).toHaveBeenCalledWith("read_markdown_file", {
      path: mockReadmePath
    });
  });

  it("resolves the canonical workspace resource root through Rust", async () => {
    mockedInvoke.mockResolvedValue("/mock-files/vault");

    await expect(resolveNativeWorkspaceResourceRoot("/mock-files/vault/note.md"))
      .resolves.toBe("/mock-files/vault");
    expect(mockedInvoke).toHaveBeenCalledWith("resolve_workspace_resource_root", {
      sourcePath: "/mock-files/vault/note.md"
    });
  });

  it("trashes workspace resources and drops malformed native result rows", async () => {
    mockedInvoke.mockResolvedValue([
      { relativePath: "assets/unused.png", status: "trashed" },
      { relativePath: "assets/failed.pdf", status: "failed", error: "changed" },
      { relativePath: "assets/unknown.bin", status: "unknown" },
      null
    ]);

    await expect(trashNativeWorkspaceResources("/mock-files/vault", [{
      modifiedAt: 100,
      relativePath: "assets/unused.png",
      sizeBytes: 42
    }])).resolves.toEqual([
      { relativePath: "assets/unused.png", status: "trashed" },
      { relativePath: "assets/failed.pdf", status: "failed", error: "changed" }
    ]);
    expect(mockedInvoke).toHaveBeenCalledWith("trash_workspace_resources", {
      resources: [{ modifiedAt: 100, relativePath: "assets/unused.png", sizeBytes: 42 }],
      rootPath: "/mock-files/vault"
    });
  });

  it("confirms moving workspace resources to the operating system trash", async () => {
    mockedConfirm.mockResolvedValue(true);

    await expect(confirmNativeWorkspaceResourceTrash({
      cancelLabel: "Cancel",
      message: "Move 2 resources to Trash?",
      okLabel: "Move to Trash"
    })).resolves.toBe(true);
    expect(mockedConfirm).toHaveBeenCalledWith("Move 2 resources to Trash?", {
      cancelLabel: "Cancel",
      kind: "warning",
      okLabel: "Move to Trash",
      title: "QingYu"
    });
  });

  it("takes markdown paths queued by native file-open requests", async () => {
    mockedInvoke.mockResolvedValue([mockReadmePath, mockDraftPath]);

    await expect(takeNativeOpenedMarkdownPaths()).resolves.toEqual([mockReadmePath, mockDraftPath]);

    expect(mockedInvoke).toHaveBeenCalledWith("take_opened_markdown_paths");
  });

  it("requests a durable primary notebook switch from an external window", async () => {
    await requestNativePrimaryNotebookSwitch("/Notes/Name ");

    expect(mockedInvoke).toHaveBeenCalledWith("request_primary_notebook_switch", {
      path: "/Notes/Name "
    });
  });

  it("listens for markdown paths opened by the operating system", async () => {
    const unlisten = vi.fn();
    const onPaths = vi.fn();
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await listenNativeOpenedMarkdownPaths(onPaths);
    const listener = mockedListen.mock.calls[0]?.[1];

    listener?.({ payload: { paths: [mockReadmePath] } } as Parameters<NonNullable<typeof listener>>[0]);
    listener?.({ payload: { paths: [] } } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("markra://opened-markdown-paths", expect.any(Function));
    expect(onPaths).toHaveBeenCalledTimes(1);
    expect(onPaths).toHaveBeenCalledWith([mockReadmePath]);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("passes localized titles to native markdown pickers", async () => {
    mockedOpen
      .mockResolvedValueOnce(mockReadmePath)
      .mockResolvedValueOnce(mockFolderPath);
    mockedInvoke
      .mockResolvedValueOnce({
        path: mockReadmePath,
        contents: "# Native"
      })
      .mockResolvedValueOnce(mockFolderPath);

    await openNativeMarkdownFile({ title: "Open Markdown file" });
    await openNativeMarkdownFolder({ title: "Open folder" });

    expect(mockedOpen).toHaveBeenNthCalledWith(1, {
      multiple: false,
      fileAccessMode: "scoped",
      filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }],
      title: "Open Markdown file"
    });
    expect(mockedOpen).toHaveBeenNthCalledWith(2, {
      multiple: false,
      directory: true,
      recursive: true,
      fileAccessMode: "scoped",
      title: "Open folder"
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "resolve_markdown_folder", {
      path: mockFolderPath
    });
  });

  it("does not read from disk when the native open dialog is canceled", async () => {
    mockedOpen.mockResolvedValue(null);

    await expect(openNativeMarkdownFile()).resolves.toBeNull();

    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("opens a QingYu settings JSON file through the native dialog", async () => {
    mockedOpen.mockResolvedValue("/mock-files/markra-settings.json");
    mockedInvoke.mockResolvedValue({
      path: "/mock-files/markra-settings.json",
      contents: "{\"format\":\"markra-settings\"}"
    });

    await expect(openNativeSettingsFile({ title: "Import QingYu settings" })).resolves.toEqual({
      path: "/mock-files/markra-settings.json",
      name: "markra-settings.json",
      content: "{\"format\":\"markra-settings\"}"
    });

    expect(mockedOpen).toHaveBeenCalledWith({
      multiple: false,
      fileAccessMode: "scoped",
      filters: [{ name: "QingYu settings", extensions: ["json"] }],
      title: "Import QingYu settings"
    });
    expect(mockedInvoke).toHaveBeenCalledWith("read_text_file", {
      path: "/mock-files/markra-settings.json"
    });
  });

  it("opens local images through the native picker and reads them as files", async () => {
    mockedOpen.mockResolvedValue([
      "/mock-files/Local Diagram.png",
      "/mock-files/chart.svg"
    ]);
    mockedInvoke
      .mockResolvedValueOnce({
        bytes: [1, 2, 3],
        mimeType: "image/png",
        path: "/mock-files/Local Diagram.png"
      })
      .mockResolvedValueOnce({
        bytes: [60, 115, 118, 103, 62],
        mimeType: "image/svg+xml",
        path: "/mock-files/chart.svg"
      });

    const images = await openNativeLocalImages({ title: "Import local images" });

    expect(images).toHaveLength(2);
    expect(images[0]).toMatchObject({
      name: "Local Diagram.png",
      type: "image/png"
    });
    expect(images[1]).toMatchObject({
      name: "chart.svg",
      type: "image/svg+xml"
    });
    await expect(images[0]?.arrayBuffer()).resolves.toEqual(new Uint8Array([1, 2, 3]).buffer);

    expect(mockedOpen).toHaveBeenCalledWith({
      multiple: true,
      fileAccessMode: "scoped",
      filters: [{ name: "Images", extensions: ["avif", "bmp", "gif", "jpeg", "jpg", "png", "svg", "webp"] }],
      title: "Import local images"
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "read_local_image_file", {
      path: "/mock-files/Local Diagram.png"
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "read_local_image_file", {
      path: "/mock-files/chart.svg"
    });
  });

  it("returns an empty image list when local image import is canceled", async () => {
    mockedOpen.mockResolvedValue(null);

    await expect(openNativeLocalImages()).resolves.toEqual([]);

    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("preserves native paths for imported local images when linking without copying", async () => {
    mockedOpen.mockResolvedValue("/mock-files/Local Diagram.png");
    mockedInvoke
      .mockResolvedValueOnce({
        bytes: [1, 2, 3],
        mimeType: "image/png",
        path: "/mock-files/Local Diagram.png"
      })
      .mockResolvedValueOnce("/mock-files/Local Diagram.png");

    const [image] = await openNativeLocalImages();

    await expect(
      saveNativeClipboardImage({
        copyToStorage: false,
        documentPath: null,
        fileName: "local-diagram.png",
        folder: "assets",
        image: image!
      })
    ).resolves.toEqual({
      alt: "Local Diagram",
      src: "file:///mock-files/Local%20Diagram.png"
    });
  });

  it("opens local files as native references without reading their contents", async () => {
    mockedOpen.mockResolvedValue("/mock-files/Reference Doc.pdf");

    await expect(openNativeLocalFiles({ title: "Import local files" })).resolves.toEqual([{
      name: "Reference Doc.pdf",
      path: "/mock-files/Reference Doc.pdf"
    }]);
    expect(mockedOpen).toHaveBeenCalledWith({
      multiple: true,
      fileAccessMode: "scoped",
      title: "Import local files"
    });
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("imports a local file by linking to its canonical encoded native path", async () => {
    mockedInvoke.mockResolvedValue("/mock-files/Reference Doc.pdf");
    await expect(importNativeLocalFile({
      copyToStorage: false,
      documentPath: null,
      file: {
        name: "Reference Doc.pdf",
        path: "/mock-files/Reference Doc.pdf"
      },
      folder: "assets"
    })).resolves.toEqual({
      label: "Reference Doc.pdf",
      src: "file:///mock-files/Reference%20Doc.pdf"
    });
    expect(mockedInvoke).toHaveBeenCalledWith("canonical_local_file_path", {
      path: "/mock-files/Reference Doc.pdf"
    });
  });

  it("imports a local file into document storage through Rust", async () => {
    mockedInvoke.mockResolvedValue({
      relativePath: "assets/Reference Doc.pdf"
    });

    await expect(
      importNativeLocalFile({
        copyToStorage: true,
        documentPath: "/mock-files/notes/Meeting.md",
        file: {
          name: "Reference Doc.pdf",
          path: "/mock-files/Reference Doc.pdf"
        },
        folder: "assets"
      })
    ).resolves.toEqual({
      label: "Reference Doc.pdf",
      src: "assets/Reference%20Doc.pdf"
    });
    expect(mockedInvoke).toHaveBeenCalledWith("import_local_file", {
      documentPath: "/mock-files/notes/Meeting.md",
      folder: "assets",
      sourcePath: "/mock-files/Reference Doc.pdf"
    });
  });

  it("resolves dropped markdown file or folder paths without opening a picker", async () => {
    mockedInvoke
      .mockResolvedValueOnce({ kind: "folder", path: mockFolderPath })
      .mockResolvedValueOnce({ kind: "file", path: mockReadmePath });

    await expect(resolveNativeMarkdownPath(mockFolderPath)).resolves.toEqual({
      kind: "folder",
      name: "vault",
      path: mockFolderPath
    });

    await expect(resolveNativeMarkdownPath(mockReadmePath)).resolves.toEqual({
      kind: "file",
      name: "readme.md",
      path: mockReadmePath
    });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "resolve_markdown_path", {
      path: mockFolderPath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "resolve_markdown_path", {
      path: mockReadmePath
    });
  });

  it("opens a markdown folder through the native directory dialog", async () => {
    mockedOpen.mockResolvedValue(mockFolderPath);
    mockedInvoke.mockResolvedValue("/canonical/vault");

    await expect(openNativeMarkdownFolder()).resolves.toEqual({
      path: "/canonical/vault",
      name: "vault"
    });

    expect(mockedOpen).toHaveBeenCalledWith({
      multiple: false,
      directory: true,
      recursive: true,
      fileAccessMode: "scoped"
    });
    expect(mockedInvoke).toHaveBeenCalledWith("resolve_markdown_folder", {
      path: mockFolderPath
    });
  });

  it("resolves a markdown folder without opening a picker", async () => {
    mockedInvoke.mockResolvedValue("/canonical/vault");

    await expect(resolveNativeMarkdownFolder("/alias/vault")).resolves.toEqual({
      path: "/canonical/vault",
      name: "vault"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("resolve_markdown_folder", {
      path: "/alias/vault"
    });
    expect(mockedOpen).not.toHaveBeenCalled();
  });

  it("uses the normalized Windows folder payload returned by the native command", async () => {
    mockedInvoke.mockResolvedValue("C:\\Notes");

    await expect(resolveNativeMarkdownFolder("\\\\?\\C:\\Notes")).resolves.toEqual({
      path: "C:\\Notes",
      name: "Notes"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("resolve_markdown_folder", {
      path: "\\\\?\\C:\\Notes"
    });
  });

  it("reads the current markdown file without opening a dialog", async () => {
    mockedInvoke.mockResolvedValue({
      path: mockReadmePath,
      contents: "# External",
      sizeBytes: 10
    });

    await expect(readNativeMarkdownFile(mockReadmePath)).resolves.toEqual({
      path: mockReadmePath,
      name: "readme.md",
      content: "# External",
      sizeBytes: 10
    });

    expect(mockedInvoke).toHaveBeenCalledWith("read_markdown_file", {
      path: mockReadmePath
    });
  });

  it("lists and reads saved markdown history through native commands", async () => {
    mockedInvoke
      .mockResolvedValueOnce([
        {
          id: "history-2",
          createdAt: 1_700_000_001_000,
          sizeBytes: 32
        },
        {
          id: "history-1",
          createdAt: 1_700_000_000_000,
          sizeBytes: 24
        }
      ])
      .mockResolvedValueOnce({
        id: "history-1",
        contents: "# Previous\n\nSynthetic content."
      });

    await expect(listNativeMarkdownFileHistory(mockReadmePath)).resolves.toEqual([
      {
        id: "history-2",
        createdAt: 1_700_000_001_000,
        sizeBytes: 32
      },
      {
        id: "history-1",
        createdAt: 1_700_000_000_000,
        sizeBytes: 24
      }
    ]);
    await expect(readNativeMarkdownFileHistory(mockReadmePath, "history-1")).resolves.toEqual({
      id: "history-1",
      contents: "# Previous\n\nSynthetic content."
    });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "list_markdown_file_history", {
      path: mockReadmePath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "read_markdown_file_history", {
      id: "history-1",
      path: mockReadmePath
    });
  });

  it("reads, writes, and deletes markdown template files from the native template directory", async () => {
    mockedInvoke.mockResolvedValueOnce({
      contents: "# Template"
    });

    await expect(readNativeMarkdownTemplateFile("standup.md")).resolves.toBe("# Template");
    await writeNativeMarkdownTemplateFile("standup.md", "# Updated");
    await deleteNativeMarkdownTemplateFile("standup.md");

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "read_markdown_template_file", {
      fileName: "standup.md"
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "write_markdown_template_file", {
      contents: "# Updated",
      fileName: "standup.md"
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, "delete_markdown_template_file", {
      fileName: "standup.md"
    });
  });

  it("lists markdown files below the current file folder", async () => {
    mockedInvoke.mockResolvedValue([
      { path: "/mock-files/docs", relativePath: "docs", createdAt: 10, modifiedAt: 20 },
      { kind: "asset", path: "/mock-files/assets/pasted-image.png", relativePath: "assets/pasted-image.png", createdAt: 30, modifiedAt: 40, sizeBytes: 512 },
      { kind: "attachment", path: "/mock-files/assets/reference.docx", relativePath: "assets/reference.docx", createdAt: 35, modifiedAt: 45, sizeBytes: 1024 },
      { path: "/mock-files/readme.md", relativePath: "readme.md", createdAt: 50, modifiedAt: 60, sizeBytes: 128 },
      { path: "/mock-files/docs/guide.md", relativePath: "docs/guide.md", createdAt: 70, modifiedAt: 80, sizeBytes: 256 }
    ]);

    await expect(listNativeMarkdownFilesForPath(mockReadmePath)).resolves.toEqual([
      { kind: "folder", path: "/mock-files/docs", name: "docs", relativePath: "docs", createdAt: 10, modifiedAt: 20 },
      {
        kind: "asset",
        path: "/mock-files/assets/pasted-image.png",
        name: "pasted-image.png",
        relativePath: "assets/pasted-image.png",
        createdAt: 30,
        modifiedAt: 40,
        sizeBytes: 512
      },
      {
        kind: "attachment",
        path: "/mock-files/assets/reference.docx",
        name: "reference.docx",
        relativePath: "assets/reference.docx",
        createdAt: 35,
        modifiedAt: 45,
        sizeBytes: 1024
      },
      { path: "/mock-files/readme.md", name: "readme.md", relativePath: "readme.md", createdAt: 50, modifiedAt: 60, sizeBytes: 128 },
      { path: "/mock-files/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md", createdAt: 70, modifiedAt: 80, sizeBytes: 256 }
    ]);

    expect(mockedInvoke).toHaveBeenCalledWith("list_markdown_files_for_path", {
      path: mockReadmePath
    });

    mockedInvoke.mockResolvedValueOnce([]);

    await expect(listNativeMarkdownFilesForPath(mockReadmePath, {
      globalIgnoreRules: "generated/",
      managedAttachmentFolder: "media/files"
    })).resolves.toEqual([]);
    expect(mockedInvoke).toHaveBeenLastCalledWith("list_markdown_files_for_path", {
      globalIgnoreRules: "generated/",
      managedAttachmentFolder: "media/files",
      path: mockReadmePath
    });
  });

  it("loads markdown files incrementally through native tree load events", async () => {
    const unlisten = vi.fn();
    let emitTreeLoad: (payload: unknown) => unknown = () => {};
    const batches: Array<Awaited<ReturnType<typeof loadNativeMarkdownFilesForPath>>> = [];

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
          requestId,
          files: [
            { path: "/mock-files/readme.md", relativePath: "readme.md", createdAt: 10, modifiedAt: 20, sizeBytes: 128 }
          ]
        });
        emitTreeLoad({
          requestId,
          files: [
            { kind: "asset", path: "/mock-files/assets/pasted-image.png", relativePath: "assets/pasted-image.png", sizeBytes: 512 }
          ],
          done: true
        });
      }

      return undefined;
    });

    await expect(loadNativeMarkdownFilesForPath(mockFolderPath, {
      globalIgnoreRules: "generated/",
      managedAttachmentFolder: "assets",
      onBatch: (files) => {
        batches.push(files);
      }
    })).resolves.toEqual([
      { path: "/mock-files/readme.md", name: "readme.md", relativePath: "readme.md", createdAt: 10, modifiedAt: 20, sizeBytes: 128 },
      {
        kind: "asset",
        path: "/mock-files/assets/pasted-image.png",
        name: "pasted-image.png",
        relativePath: "assets/pasted-image.png",
        sizeBytes: 512
      }
    ]);

    expect(mockedListen).toHaveBeenCalledWith("markra://markdown-tree-load", expect.any(Function));
    expect(mockedInvoke).toHaveBeenCalledWith("load_markdown_files_for_path", {
      globalIgnoreRules: "generated/",
      managedAttachmentFolder: "assets",
      path: mockFolderPath,
      requestId: expect.any(String)
    });
    expect(batches).toEqual([
      [{ path: "/mock-files/readme.md", name: "readme.md", relativePath: "readme.md", createdAt: 10, modifiedAt: 20, sizeBytes: 128 }],
      [{
        kind: "asset",
        path: "/mock-files/assets/pasted-image.png",
        name: "pasted-image.png",
        relativePath: "assets/pasted-image.png",
        sizeBytes: 512
      }]
    ]);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("forwards global ignore rules to native workspace search", async () => {
    mockedInvoke.mockResolvedValue({
      results: [],
      searchedFileCount: 0,
      truncated: false,
      unreadableFileCount: 0
    });

    await expect(searchNativeMarkdownFilesForPath({
      globalIgnoreRules: "generated/",
      path: mockFolderPath,
      query: "needle"
    })).resolves.toMatchObject({ searchedFileCount: 0 });

    expect(mockedInvoke).toHaveBeenCalledWith("search_markdown_files_for_path", expect.objectContaining({
      globalIgnoreRules: "generated/",
      path: mockFolderPath,
      query: "needle"
    }));
  });

  it("creates folders, creates files, moves files, renames files, and deletes files through Tauri commands", async () => {
    mockedInvoke
      .mockResolvedValueOnce({ kind: "folder", path: "/mock-files/Research", relativePath: "Research" })
      .mockResolvedValueOnce({ kind: "folder", path: "/mock-files/docs/Sprint", relativePath: "docs/Sprint" })
      .mockResolvedValueOnce({ path: "/mock-files/Daily note.md", relativePath: "Daily note.md" })
      .mockResolvedValueOnce({ path: "/mock-files/Template note.md", relativePath: "Template note.md" })
      .mockResolvedValueOnce({ path: "/mock-files/docs/readme.md", relativePath: "docs/readme.md" })
      .mockResolvedValueOnce({ path: "/mock-files/Renamed.md", relativePath: "Renamed.md" })
      .mockResolvedValueOnce(undefined);

    await expect(createNativeMarkdownTreeFolder(mockFolderPath, "Research")).resolves.toEqual({
      kind: "folder",
      name: "Research",
      path: "/mock-files/Research",
      relativePath: "Research"
    });
    await expect(createNativeMarkdownTreeFolder(mockFolderPath, "Sprint", "/mock-files/docs")).resolves.toEqual({
      kind: "folder",
      name: "Sprint",
      path: "/mock-files/docs/Sprint",
      relativePath: "docs/Sprint"
    });
    await expect(createNativeMarkdownTreeFile(mockFolderPath, "Daily note")).resolves.toEqual({
      name: "Daily note.md",
      path: "/mock-files/Daily note.md",
      relativePath: "Daily note.md"
    });
    await createNativeMarkdownTreeFile(mockFolderPath, "Template note", {
      contents: "# Template note\n\nFrom template."
    });
    await expect(moveNativeMarkdownTreeFile(mockFolderPath, mockReadmePath, "/mock-files/docs")).resolves.toEqual({
      name: "readme.md",
      path: "/mock-files/docs/readme.md",
      relativePath: "docs/readme.md"
    });
    await expect(renameNativeMarkdownTreeFile(mockFolderPath, mockReadmePath, "Renamed.md")).resolves.toEqual({
      name: "Renamed.md",
      path: "/mock-files/Renamed.md",
      relativePath: "Renamed.md"
    });
    await expect(deleteNativeMarkdownTreeFile(mockFolderPath, "/mock-files/Renamed.md")).resolves.toBeUndefined();

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "create_markdown_tree_folder", {
      folderName: "Research",
      parentPath: null,
      rootPath: mockFolderPath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "create_markdown_tree_folder", {
      folderName: "Sprint",
      parentPath: "/mock-files/docs",
      rootPath: mockFolderPath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, "create_markdown_tree_file", {
      fileName: "Daily note",
      parentPath: null,
      rootPath: mockFolderPath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(4, "create_markdown_tree_file", {
      fileName: "Template note",
      parentPath: null,
      rootPath: mockFolderPath,
      contents: "# Template note\n\nFrom template."
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(5, "move_markdown_tree_file", {
      path: mockReadmePath,
      rootPath: mockFolderPath,
      targetParentPath: "/mock-files/docs"
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(6, "rename_markdown_tree_file", {
      fileName: "Renamed.md",
      path: mockReadmePath,
      rootPath: mockFolderPath
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(7, "delete_markdown_tree_file", {
      path: "/mock-files/Renamed.md",
      rootPath: mockFolderPath
    });
  });

  it("asks for native confirmation before deleting a markdown tree file", async () => {
    mockedConfirm.mockResolvedValue(true);

    await expect(
      confirmNativeMarkdownFileDelete("README.md", {
        cancelLabel: "Cancel",
        message: "Delete this file?",
        okLabel: "Confirm"
      })
    ).resolves.toBe(true);

    expect(mockedConfirm).toHaveBeenCalledWith("Delete this file?", {
      cancelLabel: "Cancel",
      kind: "warning",
      okLabel: "Confirm",
      title: "README.md"
    });
  });

  it("asks for native confirmation before discarding unsaved markdown changes", async () => {
    mockedConfirm.mockResolvedValue(true);

    await expect(
      confirmNativeUnsavedMarkdownDocumentDiscard("draft.md", {
        cancelLabel: "Cancel",
        message: "Discard unsaved changes?",
        okLabel: "Discard"
      })
    ).resolves.toBe(true);

    expect(mockedConfirm).toHaveBeenCalledWith("Discard unsaved changes?", {
      cancelLabel: "Cancel",
      kind: "warning",
      okLabel: "Discard",
      title: "draft.md"
    });
  });

  it("saves an existing markdown file in place through Tauri", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativeMarkdownFile({
        path: mockDraftPath,
        suggestedName: "draft.md",
        contents: "# Draft"
      })
    ).resolves.toEqual({
      path: mockDraftPath,
      name: "draft.md"
    });

    expect(mockedSave).not.toHaveBeenCalled();
    expect(mockedInvoke).toHaveBeenCalledWith("write_markdown_file", {
      path: mockDraftPath,
      contents: "# Draft"
    });
  });

  it("can save an existing markdown file without creating a history snapshot", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativeMarkdownFile({
        path: mockDraftPath,
        suggestedName: "draft.md",
        contents: "# Earlier",
        historyCursorId: "history-older",
        skipHistorySnapshot: true
      })
    ).resolves.toEqual({
      path: mockDraftPath,
      name: "draft.md"
    });

    expect(mockedSave).not.toHaveBeenCalled();
    expect(mockedInvoke).toHaveBeenCalledWith("write_markdown_file", {
      path: mockDraftPath,
      contents: "# Earlier",
      historyCursorId: "history-older",
      skipHistorySnapshot: true
    });
  });

  it("asks for a native save path before writing an untitled document", async () => {
    mockedSave.mockResolvedValue(mockUntitledPath);
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativeMarkdownFile({
        path: null,
        suggestedName: "Untitled.md",
        contents: "# Untitled"
      })
    ).resolves.toEqual({
      path: mockUntitledPath,
      name: "Untitled.md"
    });

    expect(mockedSave).toHaveBeenCalledWith({
      defaultPath: "Untitled.md",
      filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }]
    });
    expect(mockedInvoke).toHaveBeenCalledWith("write_markdown_file", {
      path: mockUntitledPath,
      contents: "# Untitled"
    });
  });

  it("starts the native save dialog in the open markdown folder for an untitled document", async () => {
    mockedSave.mockResolvedValue("/mock-files/vault/Untitled.md");
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativeMarkdownFile({
        defaultDirectory: "/mock-files/vault",
        path: null,
        suggestedName: "Untitled.md",
        contents: "# Untitled"
      })
    ).resolves.toEqual({
      path: "/mock-files/vault/Untitled.md",
      name: "Untitled.md"
    });

    expect(mockedSave).toHaveBeenCalledWith({
      defaultPath: "/mock-files/vault/Untitled.md",
      filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }]
    });
    expect(mockedInvoke).toHaveBeenCalledWith("write_markdown_file", {
      path: "/mock-files/vault/Untitled.md",
      contents: "# Untitled"
    });
  });

  it("exports rendered HTML through the native save dialog", async () => {
    mockedSave.mockResolvedValue("/mock-files/draft.html");
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativeHtmlFile({
        suggestedName: "draft.html",
        contents: "<!doctype html><html><body><h1>Draft</h1></body></html>"
      })
    ).resolves.toEqual({
      path: "/mock-files/draft.html",
      name: "draft.html"
    });

    expect(mockedSave).toHaveBeenCalledWith({
      defaultPath: "draft.html",
      filters: [{ name: "HTML", extensions: ["html", "htm"] }]
    });
    expect(mockedInvoke).toHaveBeenCalledWith("write_markdown_file", {
      path: "/mock-files/draft.html",
      contents: "<!doctype html><html><body><h1>Draft</h1></body></html>"
    });
  });

  it("does not write an HTML export when the save dialog is canceled", async () => {
    mockedSave.mockResolvedValue(null);

    await expect(
      saveNativeHtmlFile({
        suggestedName: "draft.html",
        contents: "<!doctype html><html></html>"
      })
    ).resolves.toBeNull();

    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("saves a QingYu settings JSON file through the native save dialog", async () => {
    mockedSave.mockResolvedValue("/mock-files/markra-settings.json");
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativeSettingsFile({
        suggestedName: "markra-settings.json",
        contents: "{\"format\":\"markra-settings\"}"
      })
    ).resolves.toEqual({
      path: "/mock-files/markra-settings.json",
      name: "markra-settings.json"
    });

    expect(mockedSave).toHaveBeenCalledWith({
      defaultPath: "markra-settings.json",
      filters: [{ name: "QingYu settings", extensions: ["json"] }]
    });
    expect(mockedInvoke).toHaveBeenCalledWith("write_text_file", {
      path: "/mock-files/markra-settings.json",
      contents: "{\"format\":\"markra-settings\"}"
    });
  });

  it("exports rendered PDF HTML through the native save dialog", async () => {
    mockedSave.mockResolvedValue("/mock-files/draft.pdf");
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativePdfFile({
        suggestedName: "draft.pdf",
        contents: "<!doctype html><html><body><h1>Draft</h1></body></html>"
      })
    ).resolves.toEqual({
      path: "/mock-files/draft.pdf",
      name: "draft.pdf"
    });

    expect(mockedSave).toHaveBeenCalledWith({
      defaultPath: "draft.pdf",
      filters: [{ name: "PDF", extensions: ["pdf"] }]
    });
    expect(mockedInvoke).toHaveBeenCalledWith("export_pdf_file", {
      path: "/mock-files/draft.pdf",
      html: "<!doctype html><html><body><h1>Draft</h1></body></html>"
    });
  });

  it("does not write a PDF export when the save dialog is canceled", async () => {
    mockedSave.mockResolvedValue(null);

    await expect(
      saveNativePdfFile({
        suggestedName: "draft.pdf",
        contents: "<!doctype html><html></html>"
      })
    ).resolves.toBeNull();

    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("exports markdown through Pandoc with the selected native save dialog filter", async () => {
    mockedSave.mockResolvedValue("/mock-files/draft.docx");
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativePandocFile({
        documentPath: "/mock-files/draft.md",
        format: "docx",
        markdown: "# Draft\n\nReady.",
        pandocArgs: "--toc",
        pandocPath: "/opt/homebrew/bin/pandoc",
        suggestedName: "draft.docx"
      })
    ).resolves.toEqual({
      path: "/mock-files/draft.docx",
      name: "draft.docx"
    });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "check_pandoc_available", {
      pandocPath: "/opt/homebrew/bin/pandoc"
    });
    expect(mockedSave).toHaveBeenCalledWith({
      defaultPath: "draft.docx",
      filters: [{ name: "Word document", extensions: ["docx"] }]
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "export_pandoc_file", {
      documentPath: "/mock-files/draft.md",
      format: "docx",
      markdown: "# Draft\n\nReady.",
      pandocArgs: "--toc",
      pandocPath: "/opt/homebrew/bin/pandoc",
      path: "/mock-files/draft.docx"
    });
  });

  it("checks Pandoc availability before opening the native save dialog", async () => {
    mockedInvoke.mockRejectedValue(new Error("Pandoc export requires Pandoc."));

    await expect(
      saveNativePandocFile({
        documentPath: null,
        format: "docx",
        markdown: "# Draft",
        pandocArgs: "",
        pandocPath: "",
        suggestedName: "draft.docx"
      })
    ).rejects.toThrow("Pandoc export requires Pandoc.");

    expect(mockedInvoke).toHaveBeenCalledWith("check_pandoc_available", {
      pandocPath: ""
    });
    expect(mockedSave).not.toHaveBeenCalled();
  });

  it("detects the native Pandoc executable path", async () => {
    mockedInvoke.mockResolvedValue("/opt/homebrew/bin/pandoc");

    await expect(detectNativePandocPath()).resolves.toBe("/opt/homebrew/bin/pandoc");

    expect(mockedInvoke).toHaveBeenCalledWith("detect_pandoc_path");
  });

  it("returns null when native Pandoc detection finds no executable", async () => {
    mockedInvoke.mockResolvedValue(null);

    await expect(detectNativePandocPath()).resolves.toBeNull();
  });

  it("uses EPUB and LaTeX save filters for Pandoc exports", async () => {
    mockedSave.mockResolvedValueOnce("/mock-files/book.epub").mockResolvedValueOnce("/mock-files/paper.tex");
    mockedInvoke.mockResolvedValue(undefined);

    await saveNativePandocFile({
      documentPath: null,
      format: "epub",
      markdown: "# Book",
      pandocArgs: "",
      pandocPath: "",
      suggestedName: "book.epub"
    });
    await saveNativePandocFile({
      documentPath: null,
      format: "latex",
      markdown: "# Paper",
      pandocArgs: "",
      pandocPath: "",
      suggestedName: "paper.tex"
    });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "check_pandoc_available", {
      pandocPath: ""
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, "check_pandoc_available", {
      pandocPath: ""
    });
    expect(mockedSave).toHaveBeenNthCalledWith(1, {
      defaultPath: "book.epub",
      filters: [{ name: "EPUB", extensions: ["epub"] }]
    });
    expect(mockedSave).toHaveBeenNthCalledWith(2, {
      defaultPath: "paper.tex",
      filters: [{ name: "LaTeX", extensions: ["tex"] }]
    });
  });

  it("does not run Pandoc export when the save dialog is canceled", async () => {
    mockedSave.mockResolvedValue(null);
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      saveNativePandocFile({
        documentPath: null,
        format: "epub",
        markdown: "# Draft",
        pandocArgs: "",
        pandocPath: "",
        suggestedName: "draft.epub"
      })
    ).resolves.toBeNull();

    expect(mockedInvoke).toHaveBeenCalledWith("check_pandoc_available", {
      pandocPath: ""
    });
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "export_pandoc_file",
      expect.any(Object)
    );
  });

  it("saves a clipboard image next to the current markdown file through Tauri", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Screenshot 1.png", { type: "image/png" });
    mockedInvoke.mockResolvedValue({
      relativePath: "assets/pasted-image-123.png"
    });

    await expect(
      saveNativeClipboardImage({
        documentPath: mockReadmePath,
        fileName: "custom-image.png",
        folder: "assets",
        image
      })
    ).resolves.toEqual({
      alt: "Screenshot 1",
      src: "assets/pasted-image-123.png"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("save_clipboard_image", {
      bytes: [1, 2, 3],
      documentPath: mockReadmePath,
      fileName: "custom-image.png",
      folder: "assets",
      mimeType: "image/png"
    });
  });

  it("saves a primary-workspace image through the fixed lowercase assets root", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Screenshot 1.png", { type: "image/png" });
    mockedInvoke.mockResolvedValue({
      relativePath: "../assets/pasted-image.png"
    });

    await expect(
      saveNativeClipboardImage({
        documentPath: "/mock-vault/notes/day.md",
        fileName: "pasted-image.png",
        folder: "ignored-folder",
        image,
        projectRootPath: "/mock-vault"
      })
    ).resolves.toEqual({
      alt: "Screenshot 1",
      src: "../assets/pasted-image.png"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("save_clipboard_image", {
      bytes: [1, 2, 3],
      documentPath: "/mock-vault/notes/day.md",
      fileName: "pasted-image.png",
      folder: "assets",
      mimeType: "image/png",
      projectRootPath: "/mock-vault",
      sourcePath: null
    });
  });

  it("creates an absolute clipboard image link without copying through Tauri", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Screenshot 1.png", { type: "image/png" });
    Object.defineProperty(image, "path", {
      value: "C:\\mock-files\\Screenshot 1.png"
    });
    mockedInvoke.mockResolvedValue("C:\\mock-files\\Screenshot 1.png");

    await expect(
      saveNativeClipboardImage({
        copyToStorage: false,
        documentPath: null,
        fileName: "custom-image.png",
        folder: "assets",
        image
      })
    ).resolves.toEqual({
      alt: "Screenshot 1",
      src: "file:///C:/mock-files/Screenshot%201.png"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("canonical_local_file_path", {
      path: "C:\\mock-files\\Screenshot 1.png"
    });
  });

  it("canonicalizes standalone dropped image references through native file access", async () => {
    const image = new File([new Uint8Array([1])], "Alias.png", { type: "image/png" });
    Object.defineProperty(image, "path", { value: "/mock-files/link/Alias.png" });
    mockedInvoke.mockResolvedValue("/mock-files/real/Alias.png");

    await expect(saveNativeClipboardImage({
      copyToStorage: false,
      documentPath: null,
      fileName: "unused.png",
      folder: "assets",
      image
    })).resolves.toEqual({ alt: "Alias", src: "file:///mock-files/real/Alias.png" });

    expect(mockedInvoke).toHaveBeenCalledWith("canonical_local_file_path", {
      path: "/mock-files/link/Alias.png"
    });
  });

  it("saves a clipboard attachment next to the current markdown file through Tauri", async () => {
    const attachment = new File([new Uint8Array([4, 5, 6])], "Reference Doc.docx", {
      type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    });
    mockedInvoke.mockResolvedValue({
      relativePath: "assets/Reference Doc.docx"
    });

    await expect(
      saveNativeClipboardAttachment({
        attachment,
        documentPath: mockReadmePath,
        folder: "assets"
      })
    ).resolves.toEqual({
      label: "Reference Doc.docx",
      src: "assets/Reference%20Doc.docx"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("save_clipboard_attachment", {
      bytes: [4, 5, 6],
      documentPath: mockReadmePath,
      fileName: "Reference Doc.docx",
      folder: "assets"
    });
  });

  it("saves a primary-workspace attachment through the fixed lowercase assets root", async () => {
    const attachment = new File([new Uint8Array([4, 5, 6])], "Reference Doc.docx");
    Object.defineProperty(attachment, "path", {
      value: "/mock-vault/imports/Reference Doc.docx"
    });
    mockedInvoke.mockResolvedValue({ relativePath: "../assets/Reference Doc.docx" });

    await expect(saveNativeClipboardAttachment({
      attachment,
      documentPath: "/mock-vault/notes/day.md",
      folder: "ignored-folder",
      projectRootPath: "/mock-vault"
    })).resolves.toEqual({
      label: "Reference Doc.docx",
      src: "../assets/Reference%20Doc.docx"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("save_clipboard_attachment", {
      bytes: [4, 5, 6],
      documentPath: "/mock-vault/notes/day.md",
      fileName: "Reference Doc.docx",
      folder: "assets",
      projectRootPath: "/mock-vault",
      sourcePath: "/mock-vault/imports/Reference Doc.docx"
    });
  });

  it("creates an absolute clipboard attachment link without copying through Tauri", async () => {
    const attachment = new File([new Uint8Array([4, 5, 6])], "Reference Doc.docx", {
      type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    });
    Object.defineProperty(attachment, "path", {
      value: "C:\\mock-files\\Reference Doc.docx"
    });
    mockedInvoke.mockResolvedValue("C:\\mock-files\\Reference Doc.docx");

    await expect(
      saveNativeClipboardAttachment({
        attachment,
        copyToStorage: false,
        documentPath: null,
        folder: "assets"
      })
    ).resolves.toEqual({
      label: "Reference Doc.docx",
      src: "file:///C:/mock-files/Reference%20Doc.docx"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("canonical_local_file_path", {
      path: "C:\\mock-files\\Reference Doc.docx"
    });
  });

  it("downloads a web image through Tauri and returns a File", async () => {
    mockedInvoke.mockResolvedValue({
      bytes: [1, 2, 3],
      fileName: "kitten.png",
      mimeType: "image/png"
    });

    const image = await downloadNativeWebImage({ src: "https://images.example.com/kitten.png" });

    expect(image).toBeInstanceOf(File);
    expect(image.name).toBe("kitten.png");
    expect(image.type).toBe("image/png");
    await expect(image.arrayBuffer()).resolves.toEqual(new Uint8Array([1, 2, 3]).buffer);
    expect(mockedInvoke).toHaveBeenCalledWith("download_web_image", {
      request: {
        url: "https://images.example.com/kitten.png"
      }
    });
  });

  it("starts and stops a native watcher for the selected markdown path and tree", async () => {
    const unlistenFile: () => unknown = vi.fn();
    const unlistenTree: () => unknown = vi.fn();
    const onChange = vi.fn();
    const onTreeChange = vi.fn();
    let emitFileChange: (path: string) => unknown = () => {};
    let emitTreeChange: (payload: { path: string; rootPath: string }) => unknown = () => {};

    mockedListen.mockImplementation(async (event, handler) => {
      if (event === "markra://tree-changed") {
        emitTreeChange = (payload) => {
          handler({ payload } as never);
        };
        return unlistenTree;
      }

      emitFileChange = (path) => {
        handler({ payload: { path } } as never);
      };
      return unlistenFile;
    });
    mockedInvoke.mockResolvedValue(undefined);

    const unwatch = await watchNativeMarkdownFile(
      mockReadmePath,
      onChange,
      onTreeChange,
      {
        globalIgnoreRules: "generated/",
        ignoreRootPath: mockFolderPath
      }
    );

    expect(mockedListen).toHaveBeenCalledWith("markra://file-changed", expect.any(Function));
    expect(mockedListen).toHaveBeenCalledWith("markra://tree-changed", expect.any(Function));
    expect(mockedInvoke).toHaveBeenCalledWith("watch_markdown_file", {
      globalIgnoreRules: "generated/",
      ignoreRootPath: mockFolderPath,
      path: mockReadmePath
    });

    emitFileChange(mockReadmePath);
    emitFileChange("/mock-files/other.md");
    emitTreeChange({ path: "/mock-files/assets/pasted-image.png", rootPath: "/mock-files" });
    emitTreeChange({ path: "/other-vault/assets/pasted-image.png", rootPath: "/other-vault" });

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(mockReadmePath);
    expect(onTreeChange).toHaveBeenCalledTimes(1);
    expect(onTreeChange).toHaveBeenCalledWith("/mock-files/assets/pasted-image.png");

    unwatch();

    expect(unlistenFile).toHaveBeenCalledTimes(1);
    expect(unlistenTree).toHaveBeenCalledTimes(1);
    expect(mockedInvoke).toHaveBeenCalledWith("unwatch_markdown_file", {
      path: mockReadmePath
    });
  });

  it("starts and stops a native watcher for a selected markdown tree root", async () => {
    const unlistenTree: () => unknown = vi.fn();
    const onTreeChange = vi.fn();
    let emitTreeChange: (payload: { path: string; rootPath: string }) => unknown = () => {};

    mockedListen.mockImplementation(async (_event, handler) => {
      emitTreeChange = (payload) => {
        handler({ payload } as never);
      };
      return unlistenTree;
    });
    mockedInvoke.mockResolvedValue(undefined);

    const unwatch = await watchNativeMarkdownTree(
      mockFolderPath,
      onTreeChange,
      { globalIgnoreRules: "generated/" }
    );

    expect(mockedListen).toHaveBeenCalledWith("markra://tree-changed", expect.any(Function));
    expect(mockedInvoke).toHaveBeenCalledWith("watch_markdown_tree", {
      globalIgnoreRules: "generated/",
      rootPath: mockFolderPath
    });

    emitTreeChange({ path: "/mock-files/vault/docs/added.md", rootPath: mockFolderPath });
    emitTreeChange({ path: "/other-vault/docs/added.md", rootPath: "/other-vault" });

    expect(onTreeChange).toHaveBeenCalledTimes(1);
    expect(onTreeChange).toHaveBeenCalledWith("/mock-files/vault/docs/added.md");

    unwatch();

    expect(unlistenTree).toHaveBeenCalledTimes(1);
    expect(mockedInvoke).toHaveBeenCalledWith("unwatch_markdown_tree", {
      rootPath: mockFolderPath
    });
  });

  it("routes dropped markdown files and folders from the native window event", async () => {
    const unlisten = vi.fn();
    const onDrop = vi.fn();
    let emitDragDrop: (payload: unknown) => unknown = () => {};
    mockedGetCurrentWindow.mockReturnValue({
      label: "main"
    } as unknown as ReturnType<typeof getCurrentWindow>);
    mockedListen.mockImplementation(async (event, handler) => {
      if (event === "tauri://drag-drop") {
        emitDragDrop = (payload) => handler({ payload } as never);
      }

      return unlisten;
    });
    mockedInvoke
      .mockRejectedValueOnce(new Error("Unsupported path"))
      .mockResolvedValueOnce({ kind: "file", path: mockReadmePath })
      .mockResolvedValueOnce({ kind: "folder", path: mockFolderPath });

    const cleanup = await installNativeMarkdownFileDrop(onDrop);

    emitDragDrop({ paths: ["/mock-files/archive.zip", mockReadmePath] });
    emitDragDrop({ paths: [mockFolderPath] });

    await vi.waitFor(() => expect(onDrop).toHaveBeenCalledTimes(2));
    expect(onDrop).toHaveBeenNthCalledWith(1, {
      kind: "file",
      name: "readme.md",
      path: mockReadmePath
    });
    expect(onDrop).toHaveBeenNthCalledWith(2, {
      kind: "folder",
      name: "vault",
      path: mockFolderPath
    });

    cleanup();

    expect(unlisten).toHaveBeenCalledTimes(4);
  });

  it("cleans up native drag drop listeners without leaking stale Tauri listener failures", async () => {
    const cleanupByEvent = new Map<string, () => unknown>();
    mockedGetCurrentWindow.mockReturnValue({
      label: "main"
    } as unknown as ReturnType<typeof getCurrentWindow>);
    mockedListen.mockImplementation(async (event) => {
      const cleanup = vi.fn().mockRejectedValue(new Error("undefined is not an object (evaluating 'listeners[eventId].handlerId')"));
      cleanupByEvent.set(String(event), cleanup);

      return cleanup;
    });

    const cleanup = await installNativeMarkdownFileDrop(vi.fn());

    expect(onDragDropEvent).not.toHaveBeenCalled();
    expect(mockedListen).toHaveBeenCalledWith("tauri://drag-enter", expect.any(Function), {
      target: { kind: "Window", label: "main" }
    });
    expect(mockedListen).toHaveBeenCalledWith("tauri://drag-over", expect.any(Function), {
      target: { kind: "Window", label: "main" }
    });
    expect(mockedListen).toHaveBeenCalledWith("tauri://drag-drop", expect.any(Function), {
      target: { kind: "Window", label: "main" }
    });
    expect(mockedListen).toHaveBeenCalledWith("tauri://drag-leave", expect.any(Function), {
      target: { kind: "Window", label: "main" }
    });

    await expect(Promise.resolve(cleanup())).resolves.toBeUndefined();
    await expect(Promise.resolve(cleanup())).resolves.toBeUndefined();

    expect(cleanupByEvent.get("tauri://drag-enter")).toHaveBeenCalledTimes(1);
    expect(cleanupByEvent.get("tauri://drag-over")).toHaveBeenCalledTimes(1);
    expect(cleanupByEvent.get("tauri://drag-drop")).toHaveBeenCalledTimes(1);
    expect(cleanupByEvent.get("tauri://drag-leave")).toHaveBeenCalledTimes(1);
  });

  it("routes dropped image files with their native drop position", async () => {
    const unlisten = vi.fn();
    const onDrop = vi.fn();
    let emitDragDrop: (payload: unknown) => unknown = () => {};
    mockedGetCurrentWindow.mockReturnValue({
      label: "main"
    } as unknown as ReturnType<typeof getCurrentWindow>);
    mockedListen.mockImplementation(async (event, handler) => {
      if (event === "tauri://drag-drop") {
        emitDragDrop = (payload) => handler({ payload } as never);
      }

      return unlisten;
    });
    mockedInvoke.mockRejectedValueOnce(new Error("Unsupported path"));

    const cleanup = await installNativeMarkdownFileDrop(onDrop);

    emitDragDrop({
      paths: ["/mock-files/Diagram.png"],
      position: { x: 340, y: 160 }
    });

    await vi.waitFor(() => expect(onDrop).toHaveBeenCalledTimes(1));
    expect(onDrop).toHaveBeenCalledWith({
      kind: "image",
      name: "Diagram.png",
      path: "/mock-files/Diagram.png",
      point: {
        left: 340,
        top: 160
      }
    });

    cleanup();

    expect(unlisten).toHaveBeenCalledTimes(4);
  });

  it("routes dropped image file positions from tagged Tauri physical payloads", async () => {
    const unlisten = vi.fn();
    const onDrop = vi.fn();
    let emitDragDrop: (payload: unknown) => unknown = () => {};
    mockedGetCurrentWindow.mockReturnValue({
      label: "main"
    } as unknown as ReturnType<typeof getCurrentWindow>);
    mockedListen.mockImplementation(async (event, handler) => {
      if (event === "tauri://drag-drop") {
        emitDragDrop = (payload) => handler({ payload } as never);
      }

      return unlisten;
    });
    mockedInvoke.mockRejectedValueOnce(new Error("Unsupported path"));

    const cleanup = await installNativeMarkdownFileDrop(onDrop);

    emitDragDrop({
      paths: ["/mock-files/Tagged.png"],
      position: { Physical: { x: 420, y: 180 } }
    });

    await vi.waitFor(() => expect(onDrop).toHaveBeenCalledTimes(1));
    expect(onDrop).toHaveBeenCalledWith({
      kind: "image",
      name: "Tagged.png",
      path: "/mock-files/Tagged.png",
      point: {
        left: 420,
        top: 180
      }
    });

    cleanup();
  });

  it("converts physical image drop positions to logical editor coordinates", async () => {
    const originalDevicePixelRatio = window.devicePixelRatio;
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 2
    });

    try {
      const unlisten = vi.fn();
      const onDrop = vi.fn();
      let emitDragDrop: (payload: unknown) => unknown = () => {};
      mockedGetCurrentWindow.mockReturnValue({
        label: "main"
      } as unknown as ReturnType<typeof getCurrentWindow>);
      mockedListen.mockImplementation(async (event, handler) => {
        if (event === "tauri://drag-drop") {
          emitDragDrop = (payload) => handler({ payload } as never);
        }

        return unlisten;
      });
      mockedInvoke.mockRejectedValueOnce(new Error("Unsupported path"));

      const cleanup = await installNativeMarkdownFileDrop(onDrop);

      emitDragDrop({
        paths: ["/mock-files/Retina.png"],
        position: { x: 680, y: 320 }
      });

      await vi.waitFor(() => expect(onDrop).toHaveBeenCalledTimes(1));
      expect(onDrop).toHaveBeenCalledWith(expect.objectContaining({
        point: {
          left: 340,
          top: 160
        }
      }));

      cleanup();
    } finally {
      Object.defineProperty(window, "devicePixelRatio", {
        configurable: true,
        value: originalDevicePixelRatio
      });
    }
  });

  it("opens only markdown file paths in a new native window", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await openNativeMarkdownFileInNewWindow(mockReadmePath);

    expect(mockedInvoke).toHaveBeenCalledWith("open_markdown_file_in_new_window", {
      path: mockReadmePath
    });
  });

  it("opens the native containing folder for a path", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await openNativeContainingFolder(mockReadmePath);

    expect(mockedInvoke).toHaveBeenCalledWith("open_containing_folder", {
      path: mockReadmePath
    });
  });

  it("opens markdown attachments through Tauri with root and document context", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await openNativeMarkdownAttachment({
      documentPath: "/mock-files/vault/docs/note.md",
      rootPath: mockFolderPath,
      src: "../assets/Reference%20Doc.docx"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("open_markdown_attachment", {
      documentPath: "/mock-files/vault/docs/note.md",
      rootPath: mockFolderPath,
      src: "../assets/Reference%20Doc.docx"
    });
  });

  it("opens absolute markdown attachments through Tauri without a root", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await expect(
      openNativeMarkdownAttachment({
        documentPath: null,
        rootPath: null,
        src: "file:///external/Reference%20Doc.docx"
      })
    ).resolves.toBeUndefined();

    expect(mockedInvoke).toHaveBeenCalledWith("open_markdown_attachment", {
      documentPath: null,
      rootPath: null,
      src: "file:///external/Reference%20Doc.docx"
    });
  });
});
