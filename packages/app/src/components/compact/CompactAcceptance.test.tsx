import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import {
  installAppTestHarness,
  mockedCreateNativeMarkdownTreeFile,
  mockedGetStoredLanguage,
  mockedGetStoredWorkspaceState,
  mockedListNativeMarkdownFilesForPath,
  mockedMoveNativeMarkdownTreeFile,
  mockedReadNativeMarkdownFile,
  mockedRenameNativeMarkdownTreeFile,
  mockedSaveStoredWorkspaceState,
  renderApp
} from "../../test/app-harness";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests
} from "../../runtime";

installAppTestHarness();

const managedRoot = "/mobile/workspace";

function configureTrueMobileRuntime(
  resolveManagedRoot: () => Promise<string | null> = async () => managedRoot
) {
  const runtime = createDefaultAppRuntime();
  configureAppRuntime({
    ...runtime,
    platform: {
      ...runtime.platform,
      resolveFormFactor: () => "mobile"
    },
    workspace: {
      resolveManagedRoot
    }
  });
}

describe("Compact acceptance", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
  });

  it("starts an empty true-mobile workspace in the welcome state", async () => {
    configureTrueMobileRuntime();
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);

    renderApp();

    expect(await screen.findByRole("heading", { name: "Start writing" })).toBeInTheDocument();
    expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith(
      managedRoot,
      { managedAttachmentFolder: "assets" }
    );
    expect(screen.getByRole("button", { name: "New Document" })).toBeInTheDocument();
  });

  it("blocks the true-mobile UI until the managed root finishes loading", async () => {
    let finishRoot!: (root: string) => unknown;
    const pendingRoot = new Promise<string>((resolve) => {
      finishRoot = resolve;
    });
    configureTrueMobileRuntime(() => pendingRoot);
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);

    renderApp();

    expect(await screen.findByRole("status")).toHaveTextContent("Preparing your notes…");
    expect(screen.queryByRole("button", { name: "New Document" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Files" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "More" })).not.toBeInTheDocument();

    await act(async () => {
      finishRoot(managedRoot);
      await pendingRoot;
    });

    expect(await screen.findByRole("button", { name: "New Document" })).toBeInTheDocument();
  });

  it("shows the managed-root reason and retries bootstrap through the blocking page", async () => {
    const resolveManagedRoot = vi.fn()
      .mockRejectedValueOnce(new Error("App data directory is unavailable."))
      .mockResolvedValueOnce(managedRoot);
    configureTrueMobileRuntime(resolveManagedRoot);
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);

    renderApp();

    expect(await screen.findByText("A clear desk, every word softly spoken.")).toBeVisible();
    expect(screen.getByRole("heading", {
      name: "The notes folder cannot be prepared right now"
    })).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent("App data directory is unavailable.");
    expect(screen.getAllByRole("button", { name: "Try again" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "New Document" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Try again" }));

    expect(await screen.findByRole("button", { name: "New Document" })).toBeInTheDocument();
    expect(resolveManagedRoot).toHaveBeenCalledTimes(2);
  });

  it("returns to the welcome state and forgets the stored path when restore reading fails", async () => {
    configureTrueMobileRuntime();
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: "notes/missing.md",
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: ["notes/missing.md"]
    });
    const missingFile = {
      name: "missing.md",
      path: `${managedRoot}/notes/missing.md`,
      relativePath: "notes/missing.md"
    };
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([missingFile]);
    mockedReadNativeMarkdownFile.mockRejectedValue(new Error("Document could not be read."));

    renderApp();

    expect(await screen.findByRole("button", { name: "New Document" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(mockedSaveStoredWorkspaceState).toHaveBeenCalledWith(expect.objectContaining({
      filePath: null
    }));
  });

  it("creates the first true-mobile document through the current name-first file flow", async () => {
    configureTrueMobileRuntime();
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    });
    const createdFile = {
      name: "Mobile draft.md",
      path: `${managedRoot}/Mobile draft.md`,
      relativePath: "Mobile draft.md"
    };
    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([])
      .mockResolvedValue([createdFile]);
    mockedCreateNativeMarkdownTreeFile.mockResolvedValue(createdFile);
    mockedReadNativeMarkdownFile.mockResolvedValue({
      content: "",
      name: createdFile.name,
      path: createdFile.path
    });
    const prompt = vi.spyOn(window, "prompt").mockReturnValue(null);

    renderApp();

    fireEvent.click(await screen.findByRole("button", { name: "New Document" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "  Mobile draft  " }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith(managedRoot, "Mobile draft");
    expect(await screen.findByRole("heading", { name: createdFile.name })).toBeInTheDocument();
    expect(prompt).not.toHaveBeenCalled();
  });

  it("localizes the true-mobile name-first prompt", async () => {
    configureTrueMobileRuntime();
    mockedGetStoredLanguage.mockResolvedValue("zh-CN");
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    });
    const createdFile = {
      name: "移动笔记.md",
      path: `${managedRoot}/移动笔记.md`,
      relativePath: "移动笔记.md"
    };
    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([])
      .mockResolvedValue([createdFile]);
    mockedCreateNativeMarkdownTreeFile.mockResolvedValue(createdFile);
    mockedReadNativeMarkdownFile.mockResolvedValue({
      content: "",
      name: createdFile.name,
      path: createdFile.path
    });
    const prompt = vi.spyOn(window, "prompt").mockReturnValue(null);

    renderApp();

    fireEvent.click(await screen.findByRole("button", { name: "新建文档" }));
    expect(screen.getByRole("dialog", { name: "新文件名" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "新文件名" }), {
      target: { value: "移动笔记" }
    });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith(managedRoot, "移动笔记");
    expect(prompt).not.toHaveBeenCalled();
  });

  it("creates, opens, and moves a named file through full-screen Files pages before returning to the editor", async () => {
    configureTrueMobileRuntime();
    const currentFile = {
      name: "Current.md",
      path: `${managedRoot}/Current.md`,
      relativePath: "Current.md"
    };
    const archiveFolder = {
      kind: "folder" as const,
      name: "archive",
      path: `${managedRoot}/archive`,
      relativePath: "archive"
    };
    const createdFile = {
      name: "Created note.md",
      path: `${managedRoot}/Created note.md`,
      relativePath: "Created note.md"
    };
    const movedFile = {
      ...createdFile,
      path: `${managedRoot}/archive/Created note.md`,
      relativePath: "archive/Created note.md"
    };
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: currentFile.relativePath,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: [currentFile.relativePath]
    });
    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([currentFile, archiveFolder])
      .mockResolvedValue([currentFile, archiveFolder, createdFile]);
    mockedReadNativeMarkdownFile.mockImplementation(async (path) => ({
      content: path === currentFile.path ? "# Current" : "",
      name: path === currentFile.path ? currentFile.name : createdFile.name,
      path
    }));
    mockedCreateNativeMarkdownTreeFile.mockResolvedValue(createdFile);
    mockedMoveNativeMarkdownTreeFile.mockResolvedValue(movedFile);
    renderApp();

    expect(await screen.findByRole("heading", { name: currentFile.name })).toBeInTheDocument();
    const editorHistoryState = window.history.state;
    fireEvent.click(screen.getByRole("button", { name: "Files" }));

    const filesPage = await screen.findByRole("region", { name: "Files" });
    expect(filesPage.parentElement).toHaveAttribute("data-compact-page", "files");
    expect(filesPage.parentElement).toHaveClass("absolute", "inset-0");
    fireEvent.click(screen.getByRole("button", { name: "New file" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "Created note" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith(
      managedRoot,
      "Created note"
    ));
    expect(await screen.findByRole("heading", { name: createdFile.name })).toBeInTheDocument();
    await act(async () => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: editorHistoryState }));
      await Promise.resolve();
    });

    fireEvent.click(screen.getByRole("button", { name: "Files" }));
    await screen.findByRole("region", { name: "Files" });
    const filesHistoryState = window.history.state;
    fireEvent.click(await screen.findByRole("button", { name: `More actions: ${createdFile.name}` }));
    fireEvent.click(screen.getByRole("button", { name: `Move ${createdFile.name}` }));

    const movePage = await screen.findByRole("region", { name: "Move to" });
    expect(movePage.parentElement).toHaveAttribute("data-compact-page", "move-target");
    expect(movePage.parentElement).toHaveClass("absolute", "inset-0");
    fireEvent.click(screen.getByRole("button", { name: archiveFolder.name }));

    await waitFor(() => expect(mockedMoveNativeMarkdownTreeFile).toHaveBeenCalledWith(
      managedRoot,
      createdFile.path,
      archiveFolder.path
    ));
    expect(await screen.findByRole("region", { name: "Files" })).toBeInTheDocument();
    await act(async () => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: filesHistoryState }));
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await act(async () => {
      await Promise.resolve();
      window.dispatchEvent(new PopStateEvent("popstate", { state: editorHistoryState }));
      await Promise.resolve();
    });

    expect(await screen.findByRole("heading", { name: movedFile.name })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByRole("region", { name: "Files" })).not.toBeInTheDocument());
  });

  it("keeps a Compact rename failure in the dialog instead of treating a swallowed toast as success", async () => {
    configureTrueMobileRuntime();
    const currentFile = {
      name: "Current.md",
      path: `${managedRoot}/Current.md`,
      relativePath: "Current.md"
    };
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: currentFile.relativePath,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: [currentFile.relativePath]
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([currentFile]);
    mockedReadNativeMarkdownFile.mockResolvedValue({
      content: "# Current",
      name: currentFile.name,
      path: currentFile.path
    });
    mockedRenameNativeMarkdownTreeFile.mockRejectedValue(new Error("File already exists"));

    renderApp();

    expect(await screen.findByRole("heading", { name: currentFile.name })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Files" }));
    await screen.findByRole("region", { name: "Files" });
    fireEvent.click(screen.getByRole("button", { name: `More actions: ${currentFile.name}` }));
    fireEvent.click(screen.getByRole("button", { name: `Rename ${currentFile.name}` }));
    fireEvent.change(screen.getByRole("textbox", { name: `Rename ${currentFile.name}` }), {
      target: { value: "Existing.md" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A file or folder with that name already exists."
    );
    expect(screen.getByRole("dialog", { name: `Rename ${currentFile.name}` })).toBeInTheDocument();
  });
});
