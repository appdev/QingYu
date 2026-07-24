import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import { CompactFileBrowserScreen } from "./CompactFileBrowserScreen";
import type { CompactAppController } from "./types";

const markdownFiles = [
  { kind: "folder" as const, path: "/vault/docs", name: "docs", relativePath: "docs" },
  { path: "/vault/docs/guide.md", name: "guide.md", relativePath: "docs/guide.md" },
  { path: "/vault/readme.md", name: "readme.md", relativePath: "readme.md" },
  { path: "/vault/.qingyu/config.json", name: "config.json", relativePath: ".qingyu/config.json" },
  { path: "/vault/.markra-sync/state.json", name: "state.json", relativePath: ".markra-sync/state.json" }
];

function deferred() {
  let resolve!: () => undefined;
  const promise = new Promise<unknown>((resolvePromise) => {
    resolve = () => {
      resolvePromise(undefined);
      return undefined;
    };
  });
  return { promise, resolve };
}

function setup(flush = vi.fn().mockResolvedValue([]), language: CompactAppController["language"] = "en") {
  const createFile = vi.fn();
  const createFolder = vi.fn().mockResolvedValue({
    kind: "folder",
    name: "notes",
    path: "/vault/notes",
    relativePath: "notes"
  });
  const deleteFile = vi.fn();
  const moveFile = vi.fn();
  const openTreeMarkdownFile = vi.fn();
  const renameFile = vi.fn();
  const navigation = {
    pop: vi.fn().mockResolvedValue(true),
    popToEditor: vi.fn().mockResolvedValue(true),
    push: vi.fn().mockReturnValue(true)
  } as unknown as CompactNavigation;
  const controller = {
    document: {},
    files: {
      createFile,
      createFolder,
      deleteFile,
      files: markdownFiles,
      moveFile,
      openFile: openTreeMarkdownFile,
      renameFile,
      sourcePath: "/vault"
    },
    language,
    saveState: {
      flush
    }
  } as unknown as CompactAppController;

  render(<CompactFileBrowserScreen controller={controller} navigation={navigation} />);
  return {
    controller,
    createFile,
    createFolder,
    deleteFile,
    flush,
    moveFile,
    navigation,
    openTreeMarkdownFile,
    renameFile
  };
}

describe("CompactFileBrowserScreen", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("covers the Compact shell and exposes one Back action", () => {
    const { navigation } = setup();
    const browser = screen.getByRole("region", { name: "Files" });

    expect(browser).toHaveClass("absolute", "inset-0");
    expect(browser.querySelector("header")).toHaveClass("pt-[var(--compact-safe-area-top)]");
    expect(browser.querySelector('[data-compact-scroll="vertical"]'))
      .toHaveClass("pb-[calc(1rem+var(--compact-bottom-inset))]");
    expect(screen.getAllByRole("button", { name: "Back" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(navigation.pop).toHaveBeenCalledTimes(1);
  });

  it("opens a tapped file and returns to the editor", async () => {
    const { navigation, openTreeMarkdownFile } = setup();
    fireEvent.click(screen.getByRole("button", { name: "readme.md" }));

    await waitFor(() => expect(openTreeMarkdownFile).toHaveBeenCalledWith(markdownFiles[2]));
    expect(navigation.popToEditor).toHaveBeenCalledTimes(1);
  });

  it("awaits an attempted flush before switching to another document", async () => {
    const pendingFlush = deferred();
    const flush = vi.fn(() => pendingFlush.promise);
    const { navigation, openTreeMarkdownFile } = setup(flush);

    fireEvent.click(screen.getByRole("button", { name: "readme.md" }));
    expect(flush).toHaveBeenCalledWith("navigation");
    expect(openTreeMarkdownFile).not.toHaveBeenCalled();

    await act(async () => {
      pendingFlush.resolve();
      await pendingFlush.promise;
    });
    expect(openTreeMarkdownFile).toHaveBeenCalledWith(markdownFiles[2]);
    expect(navigation.popToEditor).toHaveBeenCalledTimes(1);
  });

  it("expands and collapses folders", () => {
    setup();
    expect(screen.queryByRole("button", { name: "guide.md" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "docs" }));
    expect(screen.getByRole("button", { name: "guide.md" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "docs" }));
    expect(screen.queryByRole("button", { name: "guide.md" })).not.toBeInTheDocument();
  });

  it("filters nested files through the shared tree model", () => {
    setup();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search files" }), {
      target: { value: "guide" }
    });

    expect(screen.getByRole("button", { name: "guide.md" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "readme.md" })).not.toBeInTheDocument();
  });

  it("asks for a file name in-app, cancels without mutation, then opens a created file", async () => {
    const prompt = vi.spyOn(window, "prompt").mockReturnValue(null);
    const createdFile = { path: "/vault/new.md", name: "new.md", relativePath: "new.md" };
    const { createFile, navigation, openTreeMarkdownFile } = setup();

    fireEvent.click(screen.getByRole("button", { name: "New file" }));
    const firstDialog = screen.getByRole("dialog", { name: "New file name" });
    expect(firstDialog).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(createFile).not.toHaveBeenCalled();
    await waitFor(() => expect(
      screen.queryByRole("dialog", { name: "New file name" })
    ).not.toBeInTheDocument());

    createFile.mockResolvedValueOnce(createdFile);
    fireEvent.click(screen.getByRole("button", { name: "New file" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "  new  " }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(createFile).toHaveBeenCalledWith("new", null));
    expect(openTreeMarkdownFile).toHaveBeenCalledWith(createdFile);
    expect(navigation.popToEditor).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(
      screen.queryByRole("dialog", { name: "New file name" })
    ).not.toBeInTheDocument());
    expect(prompt).not.toHaveBeenCalled();
  });

  it("creates a named folder and stays in the browser", async () => {
    const { createFolder, navigation } = setup();
    fireEvent.click(screen.getByRole("button", { name: "New folder" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New folder name" }), {
      target: { value: "  notes  " }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(createFolder).toHaveBeenCalledWith("notes", null));
    expect(navigation.popToEditor).not.toHaveBeenCalled();
  });

  it("keeps duplicate and invalid-name backend failures visible in the dialog", async () => {
    const { createFile, createFolder } = setup();

    createFolder.mockRejectedValueOnce(new Error("Folder already exists at /Users/example/vault/notes"));
    fireEvent.click(screen.getByRole("button", { name: "New folder" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New folder name" }), {
      target: { value: "notes" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A file or folder with that name already exists."
    );
    expect(screen.queryByText(/Users|example|vault/u)).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "New folder name" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    createFile.mockRejectedValueOnce(new Error("File must use .md or .markdown"));
    fireEvent.click(screen.getByRole("button", { name: "New file" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "notes.txt" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Enter a valid name.");
    expect(screen.getByRole("dialog", { name: "New file name" })).toBeInTheDocument();
  });

  it("shows a generic safe error instead of credential-bearing backend details", async () => {
    const { renameFile } = setup();
    renameFile.mockRejectedValueOnce(new Error(
      "authorization=Bearer-secret token=super-secret failed at /Users/example/vault/readme.md"
    ));

    fireEvent.click(screen.getByRole("button", { name: "More actions: readme.md" }));
    fireEvent.click(screen.getByRole("button", { name: "Rename readme.md" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Rename readme.md" }), {
      target: { value: "renamed.md" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("The file operation failed.");
    expect(screen.queryByText(/Bearer-secret|super-secret|Users|example|vault/u)).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Rename readme.md" })).toBeInTheDocument();
  });

  it("preserves the selected parent when creating inside a folder", async () => {
    const createdFile = {
      path: "/vault/docs/nested.md",
      name: "nested.md",
      relativePath: "docs/nested.md"
    };
    const { createFile } = setup();
    createFile.mockResolvedValueOnce(createdFile);

    fireEvent.click(screen.getByRole("button", { name: "More actions: docs" }));
    fireEvent.click(screen.getByRole("button", { name: "New file here" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "nested" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(createFile).toHaveBeenCalledWith("nested", "/vault/docs"));
  });

  it("uses the existing rename and delete business callbacks", async () => {
    const { deleteFile, renameFile } = setup();

    fireEvent.click(screen.getByRole("button", { name: "More actions: readme.md" }));
    fireEvent.click(screen.getByRole("button", { name: "Rename readme.md" }));
    expect(screen.getByRole("textbox", { name: "Rename readme.md" })).toHaveValue("readme.md");
    fireEvent.change(screen.getByRole("textbox", { name: "Rename readme.md" }), {
      target: { value: "renamed.md" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    await waitFor(() => expect(renameFile).toHaveBeenCalledWith(markdownFiles[2], "renamed.md"));

    fireEvent.click(screen.getByRole("button", { name: "More actions: readme.md" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete readme.md" }));
    await waitFor(() => expect(deleteFile).toHaveBeenCalledWith(markdownFiles[2]));
  });

  it("restores focus to the rename action after cancelling its dialog", async () => {
    const { renameFile } = setup();
    fireEvent.click(screen.getByRole("button", { name: "More actions: readme.md" }));
    const renameAction = screen.getByRole("button", { name: "Rename readme.md" });
    renameAction.focus();
    fireEvent.click(renameAction);

    expect(screen.getByRole("dialog", { name: "Rename readme.md" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(document.activeElement).toBe(renameAction));
    expect(renameFile).not.toHaveBeenCalled();
  });

  it("opens the full-screen move target without adding drag-and-drop", () => {
    const { navigation } = setup();
    fireEvent.click(screen.getByRole("button", { name: "More actions: readme.md" }));
    fireEvent.click(screen.getByRole("button", { name: "Move readme.md" }));

    expect(navigation.push).toHaveBeenCalledWith({ kind: "move-target", path: "/vault/readme.md" });
    expect(document.querySelector("[draggable='true']")).not.toBeInTheDocument();
  });

  it("opens actions on a held pointer and cancels on movement, up, or cancel", () => {
    vi.useFakeTimers();
    try {
      setup();
      const file = screen.getByRole("button", { name: "readme.md" });

      fireEvent.pointerDown(file);
      act(() => vi.advanceTimersByTime(500));
      expect(screen.getByRole("button", { name: "Move readme.md" })).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "More actions: readme.md" }));

      ["move", "up", "cancel"].forEach((cancelEvent) => {
        fireEvent.pointerDown(file);
        if (cancelEvent === "move") fireEvent.pointerMove(file);
        if (cancelEvent === "up") fireEvent.pointerUp(file);
        if (cancelEvent === "cancel") fireEvent.pointerCancel(file);
        act(() => vi.advanceTimersByTime(500));
        expect(screen.queryByRole("button", { name: "Move readme.md" })).not.toBeInTheDocument();
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps a completed long press latched through movement and suppresses the following click", () => {
    vi.useFakeTimers();
    try {
      const { openTreeMarkdownFile } = setup();
      const file = screen.getByRole("button", { name: "readme.md" });

      fireEvent.pointerDown(file);
      act(() => vi.advanceTimersByTime(500));
      fireEvent.pointerMove(file);
      fireEvent.click(file);

      expect(openTreeMarkdownFile).not.toHaveBeenCalled();
      expect(screen.getByRole("button", { name: "Move readme.md" })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("never renders protected metadata and gives every action a 44px target", () => {
    setup();
    expect(screen.queryByText(/\.qingyu/u)).not.toBeInTheDocument();
    expect(screen.queryByText(/\.markra-sync/u)).not.toBeInTheDocument();

    screen.getAllByRole("button").forEach((button) => {
      expect(button).toHaveClass("min-h-11");
      expect(button).toHaveClass("min-w-11");
    });
  });

  it("localizes the full file surface and name-first dialogs in Simplified Chinese", async () => {
    const { createFolder } = setup(undefined, "zh-CN");

    expect(screen.getByRole("region", { name: "文件" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "返回" })).toBeInTheDocument();
    expect(screen.getByRole("searchbox", { name: "搜索文件" })).toHaveAttribute("placeholder", "搜索文件");
    expect(screen.getByRole("button", { name: "新建文件" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "新建文件夹" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "新建文件" }));
    expect(screen.getByRole("dialog", { name: "新文件名" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));

    createFolder.mockRejectedValueOnce("failed");
    fireEvent.click(screen.getByRole("button", { name: "新建文件夹" }));
    expect(screen.getByRole("dialog", { name: "新文件夹名" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "新文件夹名" }), {
      target: { value: "资料" }
    });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("文件操作失败。");

    fireEvent.click(screen.getByRole("button", { name: "更多操作: readme.md" }));
    expect(screen.getByRole("button", { name: "重命名 readme.md" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "移动 readme.md" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "删除 readme.md" })).toBeInTheDocument();
  });
});
