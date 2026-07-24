import { useEffect } from "react";
import { act, fireEvent, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import {
  createNativeMarkdownTreeFile,
  createNativeMarkdownTreeFolder,
  deleteNativeMarkdownTreeFile,
  loadNativeMarkdownFilesForPath,
  listNativeMarkdownFilesForPath,
  moveNativeMarkdownTreeFile,
  openNativeMarkdownFolder,
  renameNativeMarkdownTreeFile,
  watchNativeMarkdownTree
} from "../lib/tauri";
import {
  getStoredFileTreeSortByWorkspace,
  getStoredWorkspaceState,
  saveStoredFileTreeSortForWorkspace,
  saveStoredWorkspaceState
} from "../lib/settings/app-settings";
import { useMarkdownFileTree } from "./useMarkdownFileTree";

vi.mock("../lib/tauri", () => ({
  createNativeMarkdownTreeFile: vi.fn(),
  createNativeMarkdownTreeFolder: vi.fn(),
  deleteNativeMarkdownTreeFile: vi.fn(),
  loadNativeMarkdownFilesForPath: vi.fn(),
  listNativeMarkdownFilesForPath: vi.fn(),
  moveNativeMarkdownTreeFile: vi.fn(),
  openNativeMarkdownFolder: vi.fn(),
  renameNativeMarkdownTreeFile: vi.fn(),
  watchNativeMarkdownTree: vi.fn()
}));

vi.mock("../lib/settings/app-settings", () => ({
  defaultStoredFileTreeSort: {
    direction: "ascending",
    key: "name"
  },
  getStoredFileTreeSortByWorkspace: vi.fn(),
  getStoredWorkspaceState: vi.fn(),
  normalizeStoredFileTreeSort: vi.fn((sort) => sort),
  saveStoredFileTreeSortForWorkspace: vi.fn(),
  saveStoredWorkspaceState: vi.fn()
}));

const mockedCreateNativeMarkdownTreeFile = vi.mocked(createNativeMarkdownTreeFile);
const mockedCreateNativeMarkdownTreeFolder = vi.mocked(createNativeMarkdownTreeFolder);
const mockedDeleteNativeMarkdownTreeFile = vi.mocked(deleteNativeMarkdownTreeFile);
const mockedLoadNativeMarkdownFilesForPath = vi.mocked(loadNativeMarkdownFilesForPath);
const mockedListNativeMarkdownFilesForPath = vi.mocked(listNativeMarkdownFilesForPath);
const mockedMoveNativeMarkdownTreeFile = vi.mocked(moveNativeMarkdownTreeFile);
const mockedOpenNativeMarkdownFolder = vi.mocked(openNativeMarkdownFolder);
const mockedRenameNativeMarkdownTreeFile = vi.mocked(renameNativeMarkdownTreeFile);
const mockedWatchNativeMarkdownTree = vi.mocked(watchNativeMarkdownTree);
const mockedGetStoredFileTreeSortByWorkspace = vi.mocked(getStoredFileTreeSortByWorkspace);
const mockedGetStoredWorkspaceState = vi.mocked(getStoredWorkspaceState);
const mockedSaveStoredFileTreeSortForWorkspace = vi.mocked(saveStoredFileTreeSortForWorkspace);
const mockedSaveStoredWorkspaceState = vi.mocked(saveStoredWorkspaceState);
type ListedMarkdownFiles = Awaited<ReturnType<typeof listNativeMarkdownFilesForPath>>;

function createDeferredMarkdownFileList() {
  let resolve!: (files: ListedMarkdownFiles) => undefined;
  const promise = new Promise<ListedMarkdownFiles>((resolvePromise) => {
    resolve = (files) => {
      resolvePromise(files);
      return undefined;
    };
  });

  return { promise, resolve };
}

function mockWorkspaceState(
  patch: Partial<Awaited<ReturnType<typeof getStoredWorkspaceState>>> = {}
): Awaited<ReturnType<typeof getStoredWorkspaceState>> {
  return {
    filePath: null,
    fileTreeOpen: false,
    folderName: null,
    folderPath: null,
    openFilePaths: [],
    openWindows: [],
    ...patch
  };
}

function FileTreeProbe({
  currentPath = null,
  globalIgnoreRules,
  managedAttachmentFolder,
  onFilesChange
}: {
  currentPath?: string | null;
  globalIgnoreRules?: string;
  managedAttachmentFolder?: string;
  onFilesChange?: (files: ReturnType<typeof useMarkdownFileTree>["files"]) => unknown;
}) {
  const tree = useMarkdownFileTree({ globalIgnoreRules, managedAttachmentFolder });

  useEffect(() => {
    onFilesChange?.(tree.files);
  }, [onFilesChange, tree.files]);

  return (
    <section>
      <p data-testid="root-name">{tree.rootNameForDocument(currentPath)}</p>
      <p data-testid="project-root">{tree.projectRoot ?? "none"}</p>
      <p data-testid="source-path">{tree.sourcePath ?? "none"}</p>
      <p data-testid="open-state">{tree.open ? "open" : "closed"}</p>
      <p data-testid="tree-width">{tree.width}</p>
      <p data-testid="tree-resizing">{tree.resizing ? "resizing" : "idle"}</p>
      <p data-testid="recent-folders-open-state">{tree.recentFoldersOpen ? "open" : "closed"}</p>
      <p data-testid="assets-visible-state">{tree.fileTreeAssetsVisible ? "visible" : "hidden"}</p>
      <p data-testid="file-tree-sort">{`${tree.fileTreeSort.key}:${tree.fileTreeSort.direction}`}</p>
      <p data-testid="layout-class">{tree.workspaceLayoutClassName}</p>
      <p data-testid="layout-columns">{tree.workspaceLayoutStyle.gridTemplateColumns}</p>
      <button type="button" onClick={() => tree.openFolderPath("/vault", "vault", true, true, { coalesce: true })}>
        Open folder
      </button>
      <button type="button">
        Cancel folder
      </button>
      <button
        type="button"
        onClick={() => tree.openFolderPath("/recent/notes", "notes", true, true, { coalesce: true })}
      >
        Open recent folder
      </button>
      <button
        type="button"
        onClick={() => tree.openFolderPath("/mock-workspaces/beta/docs", "docs", true, true, { coalesce: true })}
      >
        Open second docs folder
      </button>
      <button
        type="button"
        onClick={() => tree.openFolderPath("/vault", "vault", false, false)}
      >
        Restore collapsed folder
      </button>
      <button
        type="button"
        onClick={() => tree.openFolderPath("/vault", "vault")}
      >
        Open vault path
      </button>
      <button type="button" onClick={() => tree.setRootFromMarkdownFilePath("/mock-workspaces/notes/daily.md")}>
        Use file root
      </button>
      <button type="button" onClick={() => tree.toggle(currentPath)}>
        Toggle
      </button>
      <button type="button" onClick={() => tree.resize(512)}>
        Resize wide
      </button>
      <button type="button" onClick={() => tree.resize(120)}>
        Resize narrow
      </button>
      <button type="button" onClick={tree.startResize}>
        Start resize
      </button>
      <button type="button" onClick={tree.endResize}>
        End resize
      </button>
      <button type="button" onClick={() => tree.setRecentFoldersOpen?.(false)}>
        Collapse recent folders
      </button>
      <button type="button" onClick={() => tree.setRecentFoldersOpen?.(true)}>
        Expand recent folders
      </button>
      <button type="button" onClick={() => tree.setFileTreeAssetsVisible?.(!tree.fileTreeAssetsVisible)}>
        Toggle image assets
      </button>
      <button
        type="button"
        onClick={() => tree.setFileTreeSort({ direction: "descending", key: "createdAt" })}
      >
        Sort created descending
      </button>
      <button type="button" onClick={() => tree.createFile("Daily note")}>
        Create
      </button>
      <button type="button" onClick={() => tree.createFile("Daily note", null, "# Daily note\n")}>
        Create from template
      </button>
      <button type="button" onClick={() => tree.createFolder("Research")}>
        Create folder
      </button>
      <button type="button" onClick={() => tree.createFolder("Sprint", "/vault/docs")}>
        Create nested folder
      </button>
      <button
        type="button"
        onClick={() => tree.renameFile({ name: "readme.md", path: "/vault/readme.md", relativePath: "readme.md" }, "renamed.md")}
      >
        Rename
      </button>
      <button
        type="button"
        onClick={() =>
          tree.moveFile(
            { name: "readme.md", path: "/vault/readme.md", relativePath: "readme.md" },
            "/vault/docs"
          )}
      >
        Move
      </button>
      <button
        type="button"
        onClick={() => tree.deleteFile({ name: "renamed.md", path: "/vault/renamed.md", relativePath: "renamed.md" })}
      >
        Delete
      </button>
      <ol>
        {tree.files.map((file) => (
          <li key={file.path}>{file.relativePath}</li>
        ))}
      </ol>
    </section>
  );
}

describe("useMarkdownFileTree", () => {
  beforeEach(() => {
    mockedCreateNativeMarkdownTreeFile.mockReset();
    mockedCreateNativeMarkdownTreeFolder.mockReset();
    mockedDeleteNativeMarkdownTreeFile.mockReset();
    mockedLoadNativeMarkdownFilesForPath.mockReset();
    mockedListNativeMarkdownFilesForPath.mockReset();
    mockedMoveNativeMarkdownTreeFile.mockReset();
    mockedOpenNativeMarkdownFolder.mockReset();
    mockedRenameNativeMarkdownTreeFile.mockReset();
    mockedWatchNativeMarkdownTree.mockReset();
    mockedGetStoredFileTreeSortByWorkspace.mockReset();
    mockedGetStoredWorkspaceState.mockReset();
    mockedSaveStoredFileTreeSortForWorkspace.mockReset();
    mockedSaveStoredWorkspaceState.mockReset();
    mockedGetStoredFileTreeSortByWorkspace.mockResolvedValue({});
    mockedGetStoredWorkspaceState.mockResolvedValue(mockWorkspaceState({
      recentFoldersOpen: true
    }));
    mockedCreateNativeMarkdownTreeFile.mockResolvedValue({
      name: "Daily note.md",
      path: "/vault/Daily note.md",
      relativePath: "Daily note.md"
    });
    mockedCreateNativeMarkdownTreeFolder.mockResolvedValue({
      kind: "folder",
      name: "Research",
      path: "/vault/Research",
      relativePath: "Research"
    });
    mockedDeleteNativeMarkdownTreeFile.mockResolvedValue(undefined);
    mockedRenameNativeMarkdownTreeFile.mockResolvedValue({
      name: "renamed.md",
      path: "/vault/renamed.md",
      relativePath: "renamed.md"
    });
    mockedMoveNativeMarkdownTreeFile.mockResolvedValue({
      name: "readme.md",
      path: "/vault/docs/readme.md",
      relativePath: "docs/readme.md"
    });
    mockedSaveStoredWorkspaceState.mockResolvedValue(undefined);
    mockedSaveStoredFileTreeSortForWorkspace.mockResolvedValue(undefined);
    mockedWatchNativeMarkdownTree.mockResolvedValue(() => {});
    mockedLoadNativeMarkdownFilesForPath.mockImplementation((path, options = {}) => {
      return mockedListNativeMarkdownFilesForPath(path, {
        ...(options.globalIgnoreRules ? { globalIgnoreRules: options.globalIgnoreRules } : {}),
        managedAttachmentFolder: options.managedAttachmentFolder
      });
    });
  });

  it("opens a selected markdown folder as the tree root", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
    ]);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    expect(screen.getByTestId("root-name")).toHaveTextContent("vault");
    expect(screen.getByTestId("project-root")).toHaveTextContent("/vault");
    expect(screen.getByTestId("open-state")).toHaveTextContent("open");
    expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/vault", {
      managedAttachmentFolder: "assets"
    });
    expect(mockedSaveStoredWorkspaceState).toHaveBeenCalledWith({
      filePath: null,
      fileTreeOpen: true,
      folderName: "vault",
      folderPath: "/vault",
      openFilePaths: []
    });
  });

  it("loads and watches a managed root without exposing it as a recent or switchable workspace", async () => {
    const restoredFile = {
      path: "/mobile/workspace/notes/last.md",
      name: "last.md",
      relativePath: "notes/last.md"
    };
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([restoredFile]);

    const { result } = renderHook(() => useMarkdownFileTree());
    let openedRoot: Awaited<ReturnType<typeof result.current.openFolderPath>> = null;

    await act(async () => {
      openedRoot = await result.current.openFolderPath(
        "/mobile/workspace",
        "workspace",
        false,
        false,
        {
          managed: true,
          restoreDocumentPath: restoredFile.path
        }
      );
    });

    expect(openedRoot).toEqual({
      name: "workspace",
      path: "/mobile/workspace",
      restoreDocument: restoredFile
    });
    expect(result.current.projectRoot).toBe("/mobile/workspace");
    expect(result.current.sourcePath).toBe("/mobile/workspace");
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenCalledWith(
      "/mobile/workspace",
      expect.any(Function),
      { globalIgnoreRules: "" }
    ));
    expect(mockedOpenNativeMarkdownFolder).not.toHaveBeenCalled();
    expect(mockedSaveStoredWorkspaceState).not.toHaveBeenCalled();
  });

  it("does not expose a project root until the selected folder finishes loading", async () => {
    const folderLoad = createDeferredMarkdownFileList();
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedLoadNativeMarkdownFilesForPath.mockReturnValue(folderLoad.promise);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    await waitFor(() => expect(mockedLoadNativeMarkdownFilesForPath).toHaveBeenCalled());
    expect(screen.getByTestId("project-root")).toHaveTextContent("none");

    await act(async () => {
      folderLoad.resolve([]);
      await folderLoad.promise;
    });

    expect(screen.getByTestId("project-root")).toHaveTextContent("/vault");
  });

  it("never exposes a project root for a single opened file", () => {
    mockedLoadNativeMarkdownFilesForPath.mockResolvedValue([]);
    const { result } = renderHook(() => useMarkdownFileTree());

    act(() => result.current.setRootFromMarkdownFilePath("/notes/one.md"));

    expect(result.current.sourcePath).toBe("/notes/one.md");
    expect(result.current.projectRoot).toBeNull();
  });

  it("clears the folder project root when switching to a standalone file", async () => {
    mockedLoadNativeMarkdownFilesForPath.mockResolvedValue([]);

    const { result } = renderHook(() => useMarkdownFileTree());

    await act(async () => {
      await result.current.openFolderPath("/notes", "notes");
    });
    expect(result.current.projectRoot).toBe("/notes");

    act(() => result.current.setRootFromMarkdownFilePath("/elsewhere/one.md"));

    expect(result.current.sourcePath).toBe("/elsewhere/one.md");
    expect(result.current.projectRoot).toBeNull();
  });

  it("clears the project root for a blank workspace", async () => {
    mockedLoadNativeMarkdownFilesForPath.mockResolvedValue([{
      name: "one.md",
      path: "/notes/one.md",
      relativePath: "one.md"
    }]);

    const { result } = renderHook(() => useMarkdownFileTree());

    await act(async () => {
      await result.current.openFolderPath("/notes", "notes");
    });
    expect(result.current.projectRoot).toBe("/notes");
    expect(result.current.sourcePath).toBe("/notes");
    expect(result.current.files).toHaveLength(1);
    expect(result.current.open).toBe(true);

    act(() => result.current.clearProjectRoot());

    expect(result.current.projectRoot).toBeNull();
    expect(result.current.sourcePath).toBeNull();
    expect(result.current.files).toEqual([]);
    expect(result.current.open).toBe(false);
    expect(result.current.rootNameForDocument(null)).toBe("No folder");
  });

  it("keeps the shared primary restore record unchanged for an isolated file tree", async () => {
    mockedLoadNativeMarkdownFilesForPath.mockResolvedValue([]);
    const { result } = renderHook(() => useMarkdownFileTree({
      workspacePersistencePolicy: "isolated"
    }));

    await act(async () => {
      await result.current.openFolderPath("/external", "external");
    });
    act(() => {
      result.current.toggle();
      result.current.setRecentFoldersOpen(false);
      result.current.setFileTreeAssetsVisible(false);
    });

    expect(mockedSaveStoredWorkspaceState).not.toHaveBeenCalled();
  });

  it("cancels an active folder load when the workspace becomes blank", async () => {
    const folderLoad = createDeferredMarkdownFileList();
    const folderSignalRef: { current: AbortSignal | null } = { current: null };
    mockedLoadNativeMarkdownFilesForPath.mockImplementation((_path, options = {}) => {
      folderSignalRef.current = options.signal ?? null;
      return folderLoad.promise;
    });
    const { result } = renderHook(() => useMarkdownFileTree());
    let openPromise!: ReturnType<typeof result.current.openFolderPath>;
    act(() => {
      openPromise = result.current.openFolderPath("/pending", "pending");
    });

    await waitFor(() => expect(folderSignalRef.current).not.toBeNull());
    act(() => result.current.clearProjectRoot());

    expect(folderSignalRef.current?.aborted).toBe(true);

    await act(async () => {
      folderLoad.resolve([]);
      await folderLoad.promise;
    });

    await expect(openPromise).resolves.toBeNull();
    expect(result.current.projectRoot).toBeNull();
  });

  it("cancels a coalesced folder open when the workspace becomes blank", async () => {
    vi.useFakeTimers();
    mockedLoadNativeMarkdownFilesForPath.mockResolvedValue([]);

    try {
      const { result } = renderHook(() => useMarkdownFileTree());
      let openPromise!: ReturnType<typeof result.current.openFolderPath>;
      act(() => {
        openPromise = result.current.openFolderPath(
          "/pending",
          "pending",
          true,
          true,
          { coalesce: true }
        );
      });

      act(() => result.current.clearProjectRoot());
      await act(async () => {
        vi.runOnlyPendingTimers();
        await Promise.resolve();
      });

      await expect(openPromise).resolves.toBeNull();
      expect(mockedLoadNativeMarkdownFilesForPath).not.toHaveBeenCalled();
      expect(result.current.projectRoot).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears the previous project root when the next folder load fails", async () => {
    mockedLoadNativeMarkdownFilesForPath
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error("folder unavailable"));

    const { result } = renderHook(() => useMarkdownFileTree());

    await act(async () => {
      await result.current.openFolderPath("/notes", "notes");
    });
    expect(result.current.projectRoot).toBe("/notes");

    await act(async () => {
      await result.current.openFolderPath("/missing", "missing");
    });

    expect(result.current.projectRoot).toBeNull();
  });

  it("cannot install a project root after the file tree unmounts", async () => {
    const folderLoad = createDeferredMarkdownFileList();
    mockedLoadNativeMarkdownFilesForPath.mockReturnValue(folderLoad.promise);
    const { result, unmount } = renderHook(() => useMarkdownFileTree());
    const openPromise = result.current.openFolderPath("/notes", "notes");

    await waitFor(() => expect(mockedLoadNativeMarkdownFilesForPath).toHaveBeenCalled());
    unmount();

    await act(async () => {
      folderLoad.resolve([]);
      await folderLoad.promise;
    });

    await expect(openPromise).resolves.toBeNull();
  });

  it("streams selected folder files before the full tree load resolves", async () => {
    const folderLoad = createDeferredMarkdownFileList();
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedLoadNativeMarkdownFilesForPath.mockImplementation((_path, options = {}) => {
      options.onBatch?.([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
      ]);
      return folderLoad.promise;
    });

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    expect(screen.getByTestId("root-name")).toHaveTextContent("vault");
    expect(screen.getByTestId("open-state")).toHaveTextContent("open");

    await act(async () => {
      folderLoad.resolve([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" },
        { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" }
      ]);
      await Promise.resolve();
    });

    expect(screen.getByText("docs/guide.md")).toBeInTheDocument();
  });

  it("buffers follow-up folder load batches before refreshing the tree again", async () => {
    vi.useFakeTimers();

    const folderLoad = createDeferredMarkdownFileList();
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedLoadNativeMarkdownFilesForPath.mockImplementation((_path, options = {}) => {
      options.onBatch?.([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
      ]);
      options.onBatch?.([
        { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" }
      ]);
      options.onBatch?.([
        { path: "/vault/docs/reference.md", name: "reference.md", relativePath: "docs/reference.md" }
      ]);
      return folderLoad.promise;
    });

    try {
      render(<FileTreeProbe />);

      fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        vi.runOnlyPendingTimers();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(screen.getByText("index.md")).toBeInTheDocument();
      expect(screen.queryByText("docs/guide.md")).not.toBeInTheDocument();
      expect(screen.queryByText("docs/reference.md")).not.toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(179);
        await Promise.resolve();
      });

      expect(screen.queryByText("docs/guide.md")).not.toBeInTheDocument();
      expect(screen.queryByText("docs/reference.md")).not.toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(1);
        await Promise.resolve();
      });

      expect(screen.getByText("docs/guide.md")).toBeInTheDocument();
      expect(screen.getByText("docs/reference.md")).toBeInTheDocument();

      await act(async () => {
        folderLoad.resolve([
          { path: "/vault/index.md", name: "index.md", relativePath: "index.md" },
          { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" },
          { path: "/vault/docs/reference.md", name: "reference.md", relativePath: "docs/reference.md" }
        ]);
        await Promise.resolve();
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("skips the final file tree update when streamed batches already match the resolved files", async () => {
    const files = [
      { path: "/vault/index.md", name: "index.md", relativePath: "index.md" },
      { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" }
    ];
    const folderLoad = createDeferredMarkdownFileList();
    const onFilesChange = vi.fn();

    mockedLoadNativeMarkdownFilesForPath.mockImplementation((_path, options = {}) => {
      options.onBatch?.(files);
      return folderLoad.promise;
    });

    render(<FileTreeProbe onFilesChange={onFilesChange} />);

    fireEvent.click(screen.getByRole("button", { name: "Restore collapsed folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();

    await act(async () => {
      folderLoad.resolve(files);
      await Promise.resolve();
    });

    const nonEmptyFileUpdates = onFilesChange.mock.calls.filter(([nextFiles]) => nextFiles.length > 0);
    expect(nonEmptyFileUpdates).toHaveLength(1);
  });

  it("aborts the previous native file tree load when switching folders", async () => {
    const firstLoad = createDeferredMarkdownFileList();
    const secondLoad = createDeferredMarkdownFileList();
    const firstSignalRef: { current: AbortSignal | null } = { current: null };

    mockedLoadNativeMarkdownFilesForPath
      .mockImplementationOnce((_path, options = {}) => {
        firstSignalRef.current = options.signal ?? null;
        return firstLoad.promise;
      })
      .mockImplementationOnce(() => secondLoad.promise);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Restore collapsed folder" }));

    await waitFor(() => expect(firstSignalRef.current).not.toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Open second docs folder" }));

    expect(firstSignalRef.current?.aborted).toBe(true);
  });

  it("keeps the first selected folder load alive when switching from a file root", async () => {
    const fileRootLoad = createDeferredMarkdownFileList();
    const folderLoad = createDeferredMarkdownFileList();
    const folderSignalRef: { current: AbortSignal | null } = { current: null };

    mockedLoadNativeMarkdownFilesForPath.mockImplementation((path, options = {}) => {
      if (path === "/mock-workspaces/notes/daily.md") {
        options.signal?.addEventListener("abort", () => {
          fileRootLoad.resolve([]);
        });
        return fileRootLoad.promise;
      }

      if (path === "/vault") {
        folderSignalRef.current = options.signal ?? null;
        options.signal?.addEventListener("abort", () => {
          folderLoad.resolve([]);
        });
        return folderLoad.promise;
      }

      return Promise.resolve([]);
    });

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Use file root" }));
    await waitFor(() =>
      expect(mockedLoadNativeMarkdownFilesForPath).toHaveBeenCalledWith(
        "/mock-workspaces/notes/daily.md",
        expect.objectContaining({
          managedAttachmentFolder: "assets",
          signal: expect.any(AbortSignal)
        })
      )
    );

    fireEvent.click(screen.getByRole("button", { name: "Open vault path" }));

    await waitFor(() =>
      expect(mockedLoadNativeMarkdownFilesForPath).toHaveBeenCalledWith(
        "/vault",
        expect.objectContaining({
          managedAttachmentFolder: "assets",
          signal: expect.any(AbortSignal)
        })
      )
    );
    expect(folderSignalRef.current?.aborted).toBe(false);

    await act(async () => {
      folderLoad.resolve([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
      ]);
      await Promise.resolve();
    });

    expect(screen.getByText("index.md")).toBeInTheDocument();
    expect(screen.getByTestId("root-name")).toHaveTextContent("vault");
    expect(screen.getByTestId("open-state")).toHaveTextContent("open");
  });

  it("hides attachment files outside the managed attachment folder while keeping folders visible", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { kind: "folder", path: "/vault/assets", name: "assets", relativePath: "assets" },
      { kind: "asset", path: "/vault/assets/image.png", name: "image.png", relativePath: "assets/image.png" },
      { kind: "attachment", path: "/vault/assets/reference.docx", name: "reference.docx", relativePath: "assets/reference.docx" },
      { kind: "folder", path: "/vault/downloads", name: "downloads", relativePath: "downloads" },
      { kind: "attachment", path: "/vault/downloads/export.docx", name: "export.docx", relativePath: "downloads/export.docx" },
      { kind: "folder", path: "/vault/empty", name: "empty", relativePath: "empty" },
      { kind: "folder", path: "/vault/notes", name: "notes", relativePath: "notes" },
      { path: "/vault/notes/daily.md", name: "daily.md", relativePath: "notes/daily.md" },
      { kind: "attachment", path: "/vault/todo.txt", name: "todo.txt", relativePath: "todo.txt" }
    ]);

    render(<FileTreeProbe managedAttachmentFolder="assets" />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("assets/reference.docx")).toBeInTheDocument();
    expect(screen.getByText("assets/image.png")).toBeInTheDocument();
    expect(screen.getByText("downloads")).toBeInTheDocument();
    expect(screen.getByText("empty")).toBeInTheDocument();
    expect(screen.getByText("notes/daily.md")).toBeInTheDocument();
    expect(screen.queryByText("downloads/export.docx")).not.toBeInTheDocument();
    expect(screen.queryByText("todo.txt")).not.toBeInTheDocument();
  });

  it("uses the configured managed attachment folder when filtering attachments", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { kind: "folder", path: "/vault/assets", name: "assets", relativePath: "assets" },
      { kind: "attachment", path: "/vault/assets/reference.docx", name: "reference.docx", relativePath: "assets/reference.docx" },
      { kind: "folder", path: "/vault/media", name: "media", relativePath: "media" },
      { kind: "folder", path: "/vault/media/files", name: "files", relativePath: "media/files" },
      { kind: "attachment", path: "/vault/media/files/spec.pdf", name: "spec.pdf", relativePath: "media/files/spec.pdf" }
    ]);

    render(<FileTreeProbe managedAttachmentFolder="media/files" />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("media/files/spec.pdf")).toBeInTheDocument();
    expect(screen.getByText("media")).toBeInTheDocument();
    expect(screen.getByText("media/files")).toBeInTheDocument();
    expect(screen.getByText("assets")).toBeInTheDocument();
    expect(screen.queryByText("assets/reference.docx")).not.toBeInTheDocument();
  });

  it("refreshes files when the managed attachment folder changes", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([
        { kind: "folder", path: "/vault/assets", name: "assets", relativePath: "assets" },
        { kind: "attachment", path: "/vault/assets/reference.docx", name: "reference.docx", relativePath: "assets/reference.docx" },
        { kind: "folder", path: "/vault/media", name: "media", relativePath: "media" },
        { kind: "folder", path: "/vault/media/files", name: "files", relativePath: "media/files" }
      ])
      .mockResolvedValueOnce([
        { kind: "folder", path: "/vault/assets", name: "assets", relativePath: "assets" },
        { kind: "folder", path: "/vault/media", name: "media", relativePath: "media" },
        { kind: "folder", path: "/vault/media/files", name: "files", relativePath: "media/files" },
        { kind: "attachment", path: "/vault/media/files/spec.pdf", name: "spec.pdf", relativePath: "media/files/spec.pdf" }
      ]);

    const { rerender } = render(<FileTreeProbe managedAttachmentFolder="assets" />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("assets/reference.docx")).toBeInTheDocument();
    expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/vault", {
      managedAttachmentFolder: "assets"
    });

    rerender(<FileTreeProbe managedAttachmentFolder="media/files" />);

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/vault", {
      managedAttachmentFolder: "media/files"
    }));
    expect(await screen.findByText("media/files/spec.pdf")).toBeInTheDocument();
    expect(screen.queryByText("assets/reference.docx")).not.toBeInTheDocument();
  });

  it("reloads the tree and watcher when global ignore rules change", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);

    const { rerender } = render(<FileTreeProbe globalIgnoreRules="generated/" />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/vault", {
      globalIgnoreRules: "generated/",
      managedAttachmentFolder: "assets"
    }));
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenLastCalledWith(
      "/vault",
      expect.any(Function),
      { globalIgnoreRules: "generated/" }
    ));

    rerender(<FileTreeProbe globalIgnoreRules="drafts/" />);

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/vault", {
      globalIgnoreRules: "drafts/",
      managedAttachmentFolder: "assets"
    }));
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenLastCalledWith(
      "/vault",
      expect.any(Function),
      { globalIgnoreRules: "drafts/" }
    ));
  });

  it("refreshes the selected folder when its native tree watcher reports a nested change", async () => {
    let emitTreeChange: (path: string) => unknown | Promise<unknown> = () => {};
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
      ])
      .mockResolvedValue([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" },
        { path: "/vault/docs/added.md", name: "added.md", relativePath: "docs/added.md" }
      ]);
    mockedWatchNativeMarkdownTree.mockImplementation(async (_rootPath, onTreeChange) => {
      emitTreeChange = onTreeChange;
      return () => {};
    });

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenCalledWith(
      "/vault",
      expect.any(Function),
      { globalIgnoreRules: "" }
    ));

    const callsBeforeTreeChange = mockedListNativeMarkdownFilesForPath.mock.calls.length;
    await emitTreeChange("/vault/docs/added.md");

    await waitFor(() => {
      expect(mockedListNativeMarkdownFilesForPath.mock.calls.length).toBeGreaterThan(callsBeforeTreeChange);
    });
    expect(screen.getByText("docs/added.md")).toBeInTheDocument();
  });

  it("preserves the loaded file tree when a native watcher refresh rejects", async () => {
    let emitTreeChange: (path: string) => unknown | Promise<unknown> = () => {};
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedLoadNativeMarkdownFilesForPath
      .mockResolvedValueOnce([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" },
        { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" }
      ])
      .mockImplementationOnce((_path, options = {}) => {
        options.onBatch?.([
          { path: "/vault/docs/partial.md", name: "partial.md", relativePath: "docs/partial.md" }
        ]);

        return Promise.reject(new Error("transient file tree load failure"));
      });
    mockedWatchNativeMarkdownTree.mockImplementation(async (_rootPath, onTreeChange) => {
      emitTreeChange = onTreeChange;
      return () => {};
    });

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    expect(screen.getByText("docs/guide.md")).toBeInTheDocument();
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenCalledWith(
      "/vault",
      expect.any(Function),
      { globalIgnoreRules: "" }
    ));

    await act(async () => {
      await emitTreeChange("/vault/docs/partial.md");
    });

    expect(screen.getByText("index.md")).toBeInTheDocument();
    expect(screen.getByText("docs/guide.md")).toBeInTheDocument();
    expect(screen.queryByText("docs/partial.md")).not.toBeInTheDocument();
    expect(screen.getByTestId("root-name")).toHaveTextContent("vault");
    expect(screen.getByTestId("open-state")).toHaveTextContent("open");
  });

  it("coalesces native tree watcher refreshes while a refresh is in flight", async () => {
    let emitTreeChange: (path: string) => unknown | Promise<unknown> = () => {};
    const firstRefresh = createDeferredMarkdownFileList();
    const secondRefresh = createDeferredMarkdownFileList();
    const thirdRefresh = createDeferredMarkdownFileList();
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
      ])
      .mockReturnValueOnce(firstRefresh.promise)
      .mockReturnValueOnce(secondRefresh.promise)
      .mockReturnValueOnce(thirdRefresh.promise);
    mockedWatchNativeMarkdownTree.mockImplementation(async (_rootPath, onTreeChange) => {
      emitTreeChange = onTreeChange;
      return () => {};
    });

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenCalledWith(
      "/vault",
      expect.any(Function),
      { globalIgnoreRules: "" }
    ));

    await act(async () => {
      emitTreeChange("/vault/docs/first.md");
      emitTreeChange("/vault/docs/second.md");
      emitTreeChange("/vault/docs/third.md");
      await Promise.resolve();
    });

    expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledTimes(2);

    await act(async () => {
      firstRefresh.resolve([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" },
        { path: "/vault/docs/intermediate.md", name: "intermediate.md", relativePath: "docs/intermediate.md" }
      ]);
      await Promise.resolve();
    });

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledTimes(3));

    await act(async () => {
      secondRefresh.resolve([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" },
        { path: "/vault/docs/latest.md", name: "latest.md", relativePath: "docs/latest.md" }
      ]);
      await Promise.resolve();
    });

    await waitFor(() => expect(screen.getByText("docs/latest.md")).toBeInTheDocument());
    expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledTimes(3);
  });

  it("ignores stale native tree refreshes after switching folders", async () => {
    let emitTreeChange: (path: string) => unknown | Promise<unknown> = () => {};
    const staleVaultRefresh = createDeferredMarkdownFileList();
    const docsLoad = createDeferredMarkdownFileList();

    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
      ])
      .mockReturnValueOnce(staleVaultRefresh.promise)
      .mockReturnValueOnce(docsLoad.promise);
    mockedWatchNativeMarkdownTree.mockImplementation(async (_rootPath, onTreeChange) => {
      emitTreeChange = onTreeChange;
      return () => {};
    });

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Restore collapsed folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenCalledWith(
      "/vault",
      expect.any(Function),
      { globalIgnoreRules: "" }
    ));

    const staleRefreshPromise = emitTreeChange("/vault/index.md");
    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/vault", {
      managedAttachmentFolder: "assets"
    }));

    fireEvent.click(screen.getByRole("button", { name: "Open second docs folder" }));

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/mock-workspaces/beta/docs", {
      managedAttachmentFolder: "assets"
    }));

    await act(async () => {
      docsLoad.resolve([
        { path: "/mock-workspaces/beta/docs/current.md", name: "current.md", relativePath: "current.md" }
      ]);
      await Promise.resolve();
    });

    expect(screen.getByText("current.md")).toBeInTheDocument();

    await act(async () => {
      staleVaultRefresh.resolve([
        { path: "/vault/stale.md", name: "stale.md", relativePath: "stale.md" }
      ]);
      await staleRefreshPromise;
      await Promise.resolve();
    });

    expect(screen.queryByText("stale.md")).not.toBeInTheDocument();
    expect(screen.getByText("current.md")).toBeInTheDocument();
  });

  it("ignores previous-folder refreshes while a new folder is loading", async () => {
    let emitTreeChange: (path: string) => unknown | Promise<unknown> = () => {};
    const staleVaultRefresh = createDeferredMarkdownFileList();
    const docsLoad = createDeferredMarkdownFileList();

    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([
        { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
      ])
      .mockReturnValueOnce(staleVaultRefresh.promise)
      .mockReturnValueOnce(docsLoad.promise);
    mockedWatchNativeMarkdownTree.mockImplementation(async (_rootPath, onTreeChange) => {
      emitTreeChange = onTreeChange;
      return () => {};
    });

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Restore collapsed folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    await waitFor(() => expect(mockedWatchNativeMarkdownTree).toHaveBeenCalledWith(
      "/vault",
      expect.any(Function),
      { globalIgnoreRules: "" }
    ));

    fireEvent.click(screen.getByRole("button", { name: "Open second docs folder" }));
    const staleRefreshPromise = emitTreeChange("/vault/index.md");

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/vault", {
      managedAttachmentFolder: "assets"
    }));

    await act(async () => {
      staleVaultRefresh.resolve([
        { path: "/vault/stale.md", name: "stale.md", relativePath: "stale.md" }
      ]);
      await staleRefreshPromise;
      await Promise.resolve();
    });

    expect(screen.queryByText("stale.md")).not.toBeInTheDocument();

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenLastCalledWith("/mock-workspaces/beta/docs", {
      managedAttachmentFolder: "assets"
    }));

    await act(async () => {
      docsLoad.resolve([
        { path: "/mock-workspaces/beta/docs/current.md", name: "current.md", relativePath: "current.md" }
      ]);
      await Promise.resolve();
    });

    expect(screen.getByText("current.md")).toBeInTheDocument();
    expect(screen.queryByText("stale.md")).not.toBeInTheDocument();
  });

  it("restores and persists the recent folder section expansion state", async () => {
    mockedGetStoredWorkspaceState.mockResolvedValue(mockWorkspaceState({
      recentFoldersOpen: false
    }));

    render(<FileTreeProbe />);

    await waitFor(() => expect(screen.getByTestId("recent-folders-open-state")).toHaveTextContent("closed"));

    fireEvent.click(screen.getByRole("button", { name: "Expand recent folders" }));

    expect(screen.getByTestId("recent-folders-open-state")).toHaveTextContent("open");
    expect(mockedSaveStoredWorkspaceState).toHaveBeenCalledWith({
      recentFoldersOpen: true
    });

    fireEvent.click(screen.getByRole("button", { name: "Collapse recent folders" }));

    expect(screen.getByTestId("recent-folders-open-state")).toHaveTextContent("closed");
    expect(mockedSaveStoredWorkspaceState).toHaveBeenCalledWith({
      recentFoldersOpen: false
    });
  });

  it("restores and persists file tree image asset visibility", async () => {
    mockedGetStoredWorkspaceState.mockResolvedValue(mockWorkspaceState({
      fileTreeAssetsVisible: false
    }));

    render(<FileTreeProbe />);

    await waitFor(() => expect(screen.getByTestId("assets-visible-state")).toHaveTextContent("hidden"));

    fireEvent.click(screen.getByRole("button", { name: "Toggle image assets" }));

    expect(screen.getByTestId("assets-visible-state")).toHaveTextContent("visible");
    expect(mockedSaveStoredWorkspaceState).toHaveBeenCalledWith({
      fileTreeAssetsVisible: true
    });
  });

  it("restores and persists file tree sort per selected workspace folder", async () => {
    mockedGetStoredFileTreeSortByWorkspace.mockResolvedValue({
      "/recent/notes": {
        direction: "descending",
        key: "modifiedAt"
      }
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { path: "/recent/notes/index.md", name: "index.md", relativePath: "index.md" }
    ]);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open recent folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId("file-tree-sort")).toHaveTextContent("modifiedAt:descending")
    );

    fireEvent.click(screen.getByRole("button", { name: "Sort created descending" }));

    expect(screen.getByTestId("file-tree-sort")).toHaveTextContent("createdAt:descending");
    expect(mockedSaveStoredFileTreeSortForWorkspace).toHaveBeenCalledWith("/recent/notes", {
      direction: "descending",
      key: "createdAt"
    });
  });

  it("uses the containing folder as the file tree sort workspace for a markdown file root", async () => {
    mockedGetStoredFileTreeSortByWorkspace.mockResolvedValue({
      "/mock-workspaces/notes": {
        direction: "descending",
        key: "modifiedAt"
      }
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { path: "/mock-workspaces/notes/daily.md", name: "daily.md", relativePath: "daily.md" }
    ]);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Use file root" }));

    await waitFor(() =>
      expect(screen.getByTestId("file-tree-sort")).toHaveTextContent("modifiedAt:descending")
    );

    fireEvent.click(screen.getByRole("button", { name: "Sort created descending" }));

    expect(mockedSaveStoredFileTreeSortForWorkspace).toHaveBeenCalledWith("/mock-workspaces/notes", {
      direction: "descending",
      key: "createdAt"
    });
  });

  it("restores the main file tree expansion state from workspace settings", async () => {
    mockedGetStoredWorkspaceState.mockResolvedValue(mockWorkspaceState({
      fileTreeOpen: true
    }));

    render(<FileTreeProbe />);

    await waitFor(() => expect(screen.getByTestId("open-state")).toHaveTextContent("open"));
    expect(screen.getByTestId("layout-columns")).toHaveTextContent("288px minmax(0,1fr)");
  });

  it("opens a remembered markdown folder without showing the native picker", async () => {
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { path: "/recent/notes/index.md", name: "index.md", relativePath: "index.md" }
    ]);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open recent folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();
    expect(screen.getByTestId("root-name")).toHaveTextContent("notes");
    expect(mockedOpenNativeMarkdownFolder).not.toHaveBeenCalled();
    expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/recent/notes", {
      managedAttachmentFolder: "assets"
    });
  });

  it("coalesces rapid folder selections before starting a native folder load", async () => {
    vi.useFakeTimers();

    try {
      mockedListNativeMarkdownFilesForPath.mockResolvedValue([
        { path: "/mock-workspaces/beta/docs/current.md", name: "current.md", relativePath: "current.md" }
      ]);

      render(<FileTreeProbe />);

      fireEvent.click(screen.getByRole("button", { name: "Open recent folder" }));
      fireEvent.click(screen.getByRole("button", { name: "Open second docs folder" }));

      expect(mockedListNativeMarkdownFilesForPath).not.toHaveBeenCalled();

      await act(async () => {
        vi.advanceTimersByTime(150);
        await Promise.resolve();
      });

      expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledTimes(1);
      expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/mock-workspaces/beta/docs", {
        managedAttachmentFolder: "assets"
      });
      expect(screen.getByText("current.md")).toBeInTheDocument();
      expect(screen.getByTestId("root-name")).toHaveTextContent("docs");
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the latest folder selection when folder loads finish out of order", async () => {
    vi.useFakeTimers();
    const notesLoad = createDeferredMarkdownFileList();
    const docsLoad = createDeferredMarkdownFileList();

    try {
      mockedListNativeMarkdownFilesForPath
        .mockReturnValueOnce(notesLoad.promise)
        .mockReturnValueOnce(docsLoad.promise);

      render(<FileTreeProbe />);

      fireEvent.click(screen.getByRole("button", { name: "Open recent folder" }));

      await act(async () => {
        vi.advanceTimersByTime(150);
        await Promise.resolve();
      });

      expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/recent/notes", {
        managedAttachmentFolder: "assets"
      });

      fireEvent.click(screen.getByRole("button", { name: "Open second docs folder" }));

      await act(async () => {
        vi.advanceTimersByTime(150);
        await Promise.resolve();
      });

      expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/mock-workspaces/beta/docs", {
        managedAttachmentFolder: "assets"
      });

      await act(async () => {
        docsLoad.resolve([
          { path: "/mock-workspaces/beta/docs/current.md", name: "current.md", relativePath: "current.md" }
        ]);
        await Promise.resolve();
      });

      expect(screen.getByText("current.md")).toBeInTheDocument();
      expect(screen.getByTestId("root-name")).toHaveTextContent("docs");

      await act(async () => {
        notesLoad.resolve([
          { path: "/recent/notes/stale.md", name: "stale.md", relativePath: "stale.md" }
        ]);
        await Promise.resolve();
      });

      expect(screen.queryByText("stale.md")).not.toBeInTheDocument();
      expect(screen.getByText("current.md")).toBeInTheDocument();
      expect(screen.getByTestId("root-name")).toHaveTextContent("docs");
    } finally {
      vi.useRealTimers();
    }
  });

  it("restores a markdown folder root without reopening a collapsed tree", async () => {
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" }
    ]);

    render(<FileTreeProbe currentPath="/vault/docs/guide.md" />);

    fireEvent.click(screen.getByRole("button", { name: "Restore collapsed folder" }));

    expect(await screen.findByText("docs/guide.md")).toBeInTheDocument();
    expect(screen.getByTestId("root-name")).toHaveTextContent("vault");
    expect(screen.getByTestId("project-root")).toHaveTextContent("/vault");
    expect(screen.getByTestId("open-state")).toHaveTextContent("closed");
    expect(mockedSaveStoredWorkspaceState).toHaveBeenCalledWith({
      fileTreeOpen: false,
      folderName: "vault",
      folderPath: "/vault"
    });
  });

  it("loads an explicit folder path immediately for workspace restoration", () => {
    vi.useFakeTimers();

    try {
      mockedListNativeMarkdownFilesForPath.mockResolvedValue([
        { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" }
      ]);

      render(<FileTreeProbe currentPath="/vault/docs/guide.md" />);

      fireEvent.click(screen.getByRole("button", { name: "Restore collapsed folder" }));

      expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/vault", {
        managedAttachmentFolder: "assets"
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("creates folders inside a selected nested folder", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedCreateNativeMarkdownTreeFolder.mockResolvedValue({
      kind: "folder",
      path: "/vault/docs/Sprint",
      name: "Sprint",
      relativePath: "docs/Sprint"
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
    await waitFor(() => expect(screen.getByTestId("open-state")).toHaveTextContent("open"));

    fireEvent.click(screen.getByRole("button", { name: "Create nested folder" }));

    await waitFor(() =>
      expect(mockedCreateNativeMarkdownTreeFolder).toHaveBeenCalledWith("/vault", "Sprint", "/vault/docs")
    );
    expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/vault", {
      managedAttachmentFolder: "assets"
    });
  });

  it("refreshes from the current document path when toggled open without an explicit folder", async () => {
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { path: "/vault/readme.md", name: "readme.md", relativePath: "readme.md" }
    ]);

    render(<FileTreeProbe currentPath="/vault/readme.md" />);

    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/vault/readme.md", {
      managedAttachmentFolder: "assets"
    }));
    expect(screen.getByTestId("root-name")).toHaveTextContent("vault");
    expect(screen.getByTestId("open-state")).toHaveTextContent("open");
    expect(mockedSaveStoredWorkspaceState).toHaveBeenCalledWith({ fileTreeOpen: true });
  });

  it("reopens an already loaded file tree without immediately rescanning the folder", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([
      { path: "/vault/index.md", name: "index.md", relativePath: "index.md" }
    ]);

    render(<FileTreeProbe currentPath="/vault/index.md" />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(await screen.findByText("index.md")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));
    expect(screen.getByTestId("open-state")).toHaveTextContent("closed");

    mockedListNativeMarkdownFilesForPath.mockClear();

    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));

    expect(screen.getByTestId("open-state")).toHaveTextContent("open");
    expect(mockedListNativeMarkdownFilesForPath).not.toHaveBeenCalled();
  });

  it("tracks a resizable markdown tree width for the workspace layout", async () => {
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);

    render(<FileTreeProbe currentPath="/vault/readme.md" />);

    expect(screen.getByTestId("tree-width")).toHaveTextContent("288");
    expect(screen.getByTestId("layout-columns")).toHaveTextContent("0px minmax(0,1fr)");

    fireEvent.click(screen.getByRole("button", { name: "Toggle" }));

    await waitFor(() => expect(mockedListNativeMarkdownFilesForPath).toHaveBeenCalledWith("/vault/readme.md", {
      managedAttachmentFolder: "assets"
    }));
    expect(screen.getByTestId("layout-columns")).toHaveTextContent("288px minmax(0,1fr)");

    fireEvent.click(screen.getByRole("button", { name: "Resize wide" }));

    expect(screen.getByTestId("tree-width")).toHaveTextContent("440");
    expect(screen.getByTestId("layout-columns")).toHaveTextContent("440px minmax(0,1fr)");

    fireEvent.click(screen.getByRole("button", { name: "Resize narrow" }));

    expect(screen.getByTestId("tree-width")).toHaveTextContent("220");
    expect(screen.getByTestId("layout-columns")).toHaveTextContent("220px minmax(0,1fr)");
  });

  it("disables layout transitions while the markdown tree is being resized", () => {
    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Start resize" }));

    expect(screen.getByTestId("tree-resizing")).toHaveTextContent("resizing");
    expect(screen.getByTestId("layout-class")).toHaveTextContent("transition-none");

    fireEvent.click(screen.getByRole("button", { name: "End resize" }));

    expect(screen.getByTestId("tree-resizing")).toHaveTextContent("idle");
    expect(screen.getByTestId("layout-class")).toHaveTextContent("transition-[grid-template-columns]");
  });

  it("creates folders, creates files, moves files, renames files, and deletes files through native markdown tree operations", async () => {
    mockedOpenNativeMarkdownFolder.mockResolvedValue({
      path: "/vault",
      name: "vault"
    });
    mockedListNativeMarkdownFilesForPath
      .mockResolvedValueOnce([{ path: "/vault/readme.md", name: "readme.md", relativePath: "readme.md" }])
      .mockResolvedValue([
        { path: "/vault/renamed.md", name: "renamed.md", relativePath: "renamed.md" },
        { path: "/vault/Daily note.md", name: "Daily note.md", relativePath: "Daily note.md" }
      ]);

    render(<FileTreeProbe />);

    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));

    await screen.findByText("readme.md");

    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith("/vault", "Daily note"));

    fireEvent.click(screen.getByRole("button", { name: "Create from template" }));
    await waitFor(() =>
      expect(mockedCreateNativeMarkdownTreeFile).toHaveBeenCalledWith("/vault", "Daily note", {
        contents: "# Daily note\n",
        parentPath: null
      })
    );

    fireEvent.click(screen.getByRole("button", { name: "Create folder" }));
    await waitFor(() => expect(mockedCreateNativeMarkdownTreeFolder).toHaveBeenCalledWith("/vault", "Research"));

    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    await waitFor(() =>
      expect(mockedRenameNativeMarkdownTreeFile).toHaveBeenCalledWith("/vault", "/vault/readme.md", "renamed.md")
    );

    fireEvent.click(screen.getByRole("button", { name: "Move" }));
    await waitFor(() =>
      expect(mockedMoveNativeMarkdownTreeFile).toHaveBeenCalledWith("/vault", "/vault/readme.md", "/vault/docs")
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(mockedDeleteNativeMarkdownTreeFile).toHaveBeenCalledWith("/vault", "/vault/renamed.md"));
  });
});
