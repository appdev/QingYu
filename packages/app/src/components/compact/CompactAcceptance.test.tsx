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
  kernelWorkspaceRoot,
  resetAppRuntimeForTests
} from "../../runtime";

installAppTestHarness();

const managedRoot = kernelWorkspaceRoot;

function configureTrueMobileRuntime(
  resolveRoot: () => Promise<string> = async () => managedRoot
) {
  const runtime = createDefaultAppRuntime();
  configureAppRuntime({
    ...runtime,
    platform: {
      ...runtime.platform,
      resolveFormFactor: () => "mobile"
    },
    workspace: {
      ...runtime.workspace,
      rootPolicy: {
        canChooseLocalRoot: false,
        kind: "fixed",
        resolveRoot
      }
    }
  });
}

describe("Compact acceptance", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
  });

  it("starts an empty true-mobile workspace on Workspace Home", async () => {
    configureTrueMobileRuntime();
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    }));
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);

    renderApp();

    expect(await screen.findByRole("heading", { name: "Welcome to QingYu" })).toBeInTheDocument();
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
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    }));
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
    const resolveRoot = vi.fn()
      .mockRejectedValueOnce(new Error("App data directory is unavailable."))
      .mockResolvedValueOnce(managedRoot);
    configureTrueMobileRuntime(resolveRoot);
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    }));
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
    expect(resolveRoot).toHaveBeenCalledTimes(2);
  });

  it("returns to the welcome state and forgets the stored path when restore reading fails", async () => {
    configureTrueMobileRuntime();
    const missingPath = `${managedRoot}/notes/missing.md`;
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: missingPath,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: [missingPath]
    }));
    const missingFile = {
      name: "missing.md",
      path: missingPath,
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

  it("creates the first true-mobile document immediately with an allocated untitled name", async () => {
    configureTrueMobileRuntime();
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    }));
    const createdFile = {
      name: "Untitled 2.md",
      path: `${managedRoot}/Untitled 2.md`,
      relativePath: "Untitled 2.md"
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

    await waitFor(() => expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith(
      managedRoot,
      "Untitled.md"
    ));
    expect(screen.queryByRole("dialog", { name: "New file name" })).not.toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: createdFile.name })).toBeInTheDocument();
    expect(prompt).not.toHaveBeenCalled();
  });

  it("uses the same untitled name when the true-mobile interface is localized", async () => {
    configureTrueMobileRuntime();
    mockedGetStoredLanguage.mockResolvedValue("zh-CN");
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    }));
    const createdFile = {
      name: "Untitled 1.md",
      path: `${managedRoot}/Untitled 1.md`,
      relativePath: "Untitled 1.md"
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

    await waitFor(() => expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith(
      managedRoot,
      "Untitled.md"
    ));
    expect(screen.queryByRole("dialog", { name: "新文件名" })).not.toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: createdFile.name })).toBeInTheDocument();
    expect(prompt).not.toHaveBeenCalled();
  });

  it("creates, opens, and moves an allocated untitled file through full-screen Files pages", async () => {
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
      name: "Untitled 3.md",
      path: `${managedRoot}/Untitled 3.md`,
      relativePath: "Untitled 3.md"
    };
    const movedFile = {
      ...createdFile,
      path: `${managedRoot}/archive/Untitled 3.md`,
      relativePath: "archive/Untitled 3.md"
    };
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: currentFile.path,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: [currentFile.path]
    }));
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

    await waitFor(() => expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith(
      managedRoot,
      "Untitled.md"
    ));
    expect(screen.queryByRole("dialog", { name: "New file name" })).not.toBeInTheDocument();
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
    mockedGetStoredWorkspaceState.mockImplementation(async () => ({
      filePath: currentFile.path,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: [currentFile.path]
    }));
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
