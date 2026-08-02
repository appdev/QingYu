import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { useCallback, useState } from "react";
import { DocumentTitleEditor } from "../components/DocumentTitleEditor";
import type { NativeMarkdownFolderFile, SavedNativeMarkdownFile } from "../lib/tauri";
import type { MarkdownDocumentTab } from "./useMarkdownDocument";
import {
  useDocumentTitleController,
  type UseDocumentTitleControllerOptions
} from "./useDocumentTitleController";

type RenameMarkdownTreeFile = UseDocumentTitleControllerOptions["renameMarkdownTreeFile"];
type SaveMarkdownTabContentById = UseDocumentTitleControllerOptions["saveMarkdownTabContentById"];

type TestOperations = {
  applyRenamedTreeFile: ReturnType<typeof vi.fn<UseDocumentTitleControllerOptions["applyRenamedTreeFile"]>>;
  handleMarkdownTabChange: ReturnType<typeof vi.fn<UseDocumentTitleControllerOptions["handleMarkdownTabChange"]>>;
  renameMarkdownTreeFile: ReturnType<typeof vi.fn<RenameMarkdownTreeFile>>;
  saveMarkdownTabContentById: ReturnType<typeof vi.fn<SaveMarkdownTabContentById>>;
};

function markdownTab(overrides: Partial<MarkdownDocumentTab> = {}): MarkdownDocumentTab {
  return {
    content: "---\ntitle: Notes\n---\n\n# Body\n",
    deleted: false,
    dirty: false,
    id: "notes-tab",
    name: "Notes.md",
    open: true,
    path: "/vault/Notes.md",
    revision: 7,
    ...overrides
  };
}

function renamedFile(file: NativeMarkdownFolderFile, name: string): NativeMarkdownFolderFile {
  const path = file.path.replace(/[^/\\]+$/u, name);
  return {
    ...file,
    name,
    path,
    relativePath: path
  };
}

function createOperations(): TestOperations {
  const renameMarkdownTreeFile = vi.fn<RenameMarkdownTreeFile>(async (file, fileName) => (
    renamedFile(file, fileName)
  ));
  const saveMarkdownTabContentById = vi.fn<SaveMarkdownTabContentById>(
    async (_tabId, _source): Promise<SavedNativeMarkdownFile | null> => null
  );

  return {
    applyRenamedTreeFile: vi.fn<UseDocumentTitleControllerOptions["applyRenamedTreeFile"]>(),
    handleMarkdownTabChange: vi.fn<UseDocumentTitleControllerOptions["handleMarkdownTabChange"]>(),
    renameMarkdownTreeFile,
    saveMarkdownTabContentById
  };
}

function renderController(
  initialTabs: MarkdownDocumentTab[],
  operations: TestOperations = createOperations(),
  readOnlyPaths: ReadonlySet<string> = new Set(),
  onRouteBeforeState?: (tabId: string, source: string) => unknown,
  onFailure?: (failure: { reason: string; tabId: string }) => unknown
) {
  const rendered = renderHook(() => {
    const [tabs, setTabs] = useState(initialTabs);
    const applyRenamedTreeFile = useCallback((previousPath: string, file: NativeMarkdownFolderFile) => {
      operations.applyRenamedTreeFile(previousPath, file);
      setTabs((currentTabs) => currentTabs.map((tab) => tab.path === previousPath
        ? { ...tab, deleted: false, name: file.name, path: file.path }
        : tab));
    }, []);
    const handleMarkdownTabChange = useCallback((
      tabId: string,
      source: string,
      options: { documentRevision: number; surface: "source" }
    ) => {
      operations.handleMarkdownTabChange(tabId, source, options);
      onRouteBeforeState?.(tabId, source);
      setTabs((currentTabs) => currentTabs.map((tab) => tab.id === tabId
        ? { ...tab, content: source, dirty: true }
        : tab));
    }, []);
    const saveMarkdownTabContentById = useCallback<SaveMarkdownTabContentById>(async (
      tabId,
      source,
      options
    ) => {
      const saved = await operations.saveMarkdownTabContentById(tabId, source, options);
      if (!saved) return null;

      setTabs((currentTabs) => currentTabs.map((tab) => tab.id === tabId
        ? { ...tab, content: source, dirty: false, name: saved.name, path: saved.path }
        : tab));
      return saved;
    }, []);
    const controller = useDocumentTitleController({
      applyRenamedTreeFile,
      handleMarkdownTabChange,
      isReadOnlyPath: (path) => path !== null && readOnlyPaths.has(path),
      language: "en",
      onFailure,
      renameMarkdownTreeFile: operations.renameMarkdownTreeFile,
      saveMarkdownTabContentById,
      tabs
    });

    return { controller, setTabs, tabs };
  });

  return { ...rendered, operations };
}

function ControllerTitleEditorHarness({ operations }: { operations: TestOperations }) {
  const [tabs, setTabs] = useState([markdownTab()]);
  const applyRenamedTreeFile = useCallback((previousPath: string, file: NativeMarkdownFolderFile) => {
    operations.applyRenamedTreeFile(previousPath, file);
    setTabs((currentTabs) => currentTabs.map((tab) => tab.path === previousPath
      ? { ...tab, name: file.name, path: file.path }
      : tab));
  }, []);
  const handleMarkdownTabChange = useCallback((
    tabId: string,
    source: string,
    options: { documentRevision: number; surface: "source" }
  ) => {
    operations.handleMarkdownTabChange(tabId, source, options);
    setTabs((currentTabs) => currentTabs.map((tab) => tab.id === tabId
      ? { ...tab, content: source }
      : tab));
  }, []);
  const controller = useDocumentTitleController({
    applyRenamedTreeFile,
    handleMarkdownTabChange,
    isReadOnlyPath: () => false,
    language: "en",
    renameMarkdownTreeFile: operations.renameMarkdownTreeFile,
    saveMarkdownTabContentById: operations.saveMarkdownTabContentById,
    tabs
  });
  const model = controller.modelForTab("notes-tab");

  return model ? (
    <>
      <DocumentTitleEditor language="en" {...model} />
      <button
        type="button"
        onClick={() => controller.handleSourceTitleChange(
          "notes-tab",
          tabs[0]?.content ?? "",
          "---\ntag: repair edit\n---\n\nRepaired body\n"
        )}
      >
        Remove source title
      </button>
      <output data-testid="controller-model-title">{model.title}</output>
      <output data-testid="controller-file-name">{tabs[0]?.name}</output>
      <output data-testid="controller-source">{tabs[0]?.content}</output>
    </>
  ) : null;
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => unknown;
  let reject!: (reason?: unknown) => unknown;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });

  return { promise, reject, resolve };
}

async function settle() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("useDocumentTitleController", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("repairs missing metadata with an immediate YAML title save", async () => {
    const { result, operations } = renderController([
      markdownTab({ content: "# Body\n" })
    ]);

    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });

    const expectedSource = "---\ntitle: Notes\n---\n\n# Body\n";
    expect(result.current.tabs[0]?.content).toBe(expectedSource);
    expect(operations.handleMarkdownTabChange).toHaveBeenCalledWith(
      "notes-tab",
      expectedSource,
      { documentRevision: 7, surface: "source" }
    );
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledWith(
      "notes-tab",
      expectedSource,
      { skipHistorySnapshot: true }
    );
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
  });

  it.each([
    {
      format: "YAML",
      source: "---\ntitle: Old\ntag: keep\n---\n\nBody\n",
      expected: "---\ntitle: Notes\ntag: keep\n---\n\nBody\n"
    },
    {
      format: "TOML",
      source: "+++\ntitle = \"Old\"\ntag = \"keep\"\n+++\n\nBody\n",
      expected: "+++\ntitle = \"Notes\"\ntag = \"keep\"\n+++\n\nBody\n"
    },
    {
      format: "JSON",
      source: "{\n  \"title\": \"Old\",\n  \"tag\": \"keep\"\n}\n\nBody\n",
      expected: "{\n  \"title\": \"Notes\",\n  \"tag\": \"keep\"\n}\n\nBody\n"
    }
  ])("repairs stale $format metadata to the actual filename stem", async ({ source, expected }) => {
    const { result, operations } = renderController([markdownTab({ content: source })]);

    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });

    expect(result.current.tabs[0]?.content).toBe(expected);
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledWith(
      "notes-tab",
      expected,
      { skipHistorySnapshot: true }
    );
  });

  it("does not write matching metadata", async () => {
    const { result, operations } = renderController([markdownTab()]);

    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });

    expect(operations.handleMarkdownTabChange).not.toHaveBeenCalled();
    expect(operations.saveMarkdownTabContentById).not.toHaveBeenCalled();
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
    expect(result.current.controller.modelForTab("notes-tab")).toMatchObject({
      disabled: false,
      title: "Notes"
    });
  });

  it("blocks malformed and read-only documents without rewriting either source", async () => {
    const malformed = markdownTab({
      content: "---\ntitle: [broken\n---\n",
      id: "malformed-tab",
      name: "Malformed.md",
      path: "/vault/Malformed.md"
    });
    const readOnly = markdownTab({
      content: "---\ntitle: Stale\n---\n",
      id: "readonly-tab",
      name: "Read only.md",
      path: "/vault/Read only.md"
    });
    const { result, operations } = renderController(
      [malformed, readOnly],
      createOperations(),
      new Set(["/vault/Read only.md"])
    );

    await act(async () => {
      await Promise.all([
        result.current.controller.reconcileOpenDocument("malformed-tab"),
        result.current.controller.reconcileOpenDocument("readonly-tab")
      ]);
    });

    expect(result.current.controller.modelForTab("malformed-tab")).toMatchObject({
      disabled: true,
      title: "Malformed"
    });
    expect(result.current.controller.modelForTab("readonly-tab")).toMatchObject({
      disabled: true,
      title: "Read only"
    });
    expect(operations.handleMarkdownTabChange).not.toHaveBeenCalled();
    expect(operations.saveMarkdownTabContentById).not.toHaveBeenCalled();
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
  });

  it("repairs an external stale metadata change after an earlier matching load", async () => {
    const { result, operations } = renderController([markdownTab()]);

    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });
    act(() => {
      result.current.setTabs((tabs) => tabs.map((tab) => tab.id === "notes-tab"
        ? { ...tab, content: "---\ntitle: External\n---\n\n# Changed\n", revision: 8 }
        : tab));
    });
    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });

    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledTimes(1);
    expect(result.current.tabs[0]?.content).toBe("---\ntitle: Notes\n---\n\n# Changed\n");
  });

  it("repairs repeated external metadata drift after an earlier repair", async () => {
    const { result, operations } = renderController([
      markdownTab({ content: "---\ntitle: First external\n---\n\n# First body\n" })
    ]);

    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });
    expect(result.current.tabs[0]?.content).toBe(
      "---\ntitle: Notes\n---\n\n# First body\n"
    );
    act(() => {
      result.current.setTabs((tabs) => tabs.map((tab) => tab.id === "notes-tab"
        ? {
            ...tab,
            content: "---\ntitle: Second external\n---\n\n# Second body\n",
            revision: 8
          }
        : tab));
    });
    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });

    expect(result.current.tabs[0]?.content).toBe(
      "---\ntitle: Notes\n---\n\n# Second body\n"
    );
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledTimes(2);
  });

  it("uses a 256 ms trailing debounce for visual title input", async () => {
    const { result, operations } = renderController([markdownTab()]);
    const model = result.current.controller.modelForTab("notes-tab");

    act(() => model?.onInput("Draft"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(255);
    });
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    await settle();

    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledWith(
      expect.objectContaining({ name: "Notes.md", path: "/vault/Notes.md" }),
      "Draft.md"
    );
    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledTimes(1);
  });

  it.each(["blur", "enter"] as const)("flushes a pending visual title on %s", async (reason) => {
    const { result, operations } = renderController([markdownTab()]);
    const model = result.current.controller.modelForTab("notes-tab");

    act(() => {
      model?.onInput("Flushed");
      model?.onCommit(reason);
    });
    await settle();

    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledTimes(1);
    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledWith(
      expect.any(Object),
      "Flushed.md"
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledTimes(1);
  });

  it("normalizes the visual title before requesting a rename", async () => {
    const { result, operations } = renderController([markdownTab()]);
    const model = result.current.controller.modelForTab("notes-tab");

    act(() => {
      model?.onInput("  New/Title?.  ");
      model?.onCommit("enter");
    });
    await settle();

    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledWith(
      expect.any(Object),
      "New／Title？.md"
    );
  });

  it("uses the runtime-returned filename for the model and metadata", async () => {
    const operations = createOperations();
    operations.renameMarkdownTreeFile.mockImplementation(async (file) => renamedFile(file, "Draft 1.md"));
    operations.saveMarkdownTabContentById.mockResolvedValue({
      name: "Draft 1.md",
      path: "/vault/Draft 1.md"
    });
    const { result } = renderController([markdownTab()], operations);

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("Draft");
      model?.onCommit("blur");
    });
    await settle();

    expect(result.current.controller.modelForTab("notes-tab")?.title).toBe("Draft 1");
    expect(result.current.tabs[0]?.content).toContain("title: Draft 1");
    expect(operations.applyRenamedTreeFile).toHaveBeenCalledWith(
      "/vault/Notes.md",
      expect.objectContaining({ name: "Draft 1.md", path: "/vault/Draft 1.md" })
    );
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledWith(
      "notes-tab",
      expect.stringContaining("title: Draft 1"),
      { skipHistorySnapshot: true }
    );
  });

  it("retires a caught-up runtime identity before a later external rename", async () => {
    const { result, operations } = renderController([markdownTab()]);

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("Runtime name");
      model?.onCommit("blur");
    });
    await settle();
    expect(result.current.controller.modelForTab("notes-tab")?.title).toBe("Runtime name");

    operations.handleMarkdownTabChange.mockClear();
    operations.saveMarkdownTabContentById.mockClear();
    act(() => {
      result.current.setTabs((tabs) => tabs.map((tab) => tab.id === "notes-tab"
        ? {
            ...tab,
            content: "---\ntitle: Runtime name\n---\n\nExternal body\n",
            name: "Notes.md",
            path: "/vault/Notes.md",
            revision: 8
          }
        : tab));
    });

    expect(result.current.controller.modelForTab("notes-tab")?.title).toBe("Notes");
    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });
    expect(result.current.tabs[0]?.content).toBe(
      "---\ntitle: Notes\n---\n\nExternal body\n"
    );
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledTimes(1);
  });

  it("discards runtime identity when a tab id closes and reopens at the previous path", async () => {
    const { result, operations } = renderController([markdownTab()]);

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("Runtime name");
      model?.onCommit("blur");
    });
    await settle();
    act(() => {
      result.current.setTabs((tabs) => tabs.map((tab) => tab.id === "notes-tab"
        ? { ...tab, open: false }
        : tab));
    });
    expect(result.current.controller.modelForTab("notes-tab")).toBeNull();

    operations.handleMarkdownTabChange.mockClear();
    operations.saveMarkdownTabContentById.mockClear();
    act(() => {
      result.current.setTabs((tabs) => tabs.map((tab) => tab.id === "notes-tab"
        ? {
            ...tab,
            content: "---\ntitle: Runtime name\n---\n\nReopened body\n",
            name: "Notes.md",
            open: true,
            path: "/vault/Notes.md",
            revision: 9
          }
        : tab));
    });

    expect(result.current.controller.modelForTab("notes-tab")?.title).toBe("Notes");
    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });
    expect(result.current.tabs[0]?.content).toBe(
      "---\ntitle: Notes\n---\n\nReopened body\n"
    );
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledTimes(1);
  });

  it.each([
    { label: "collision", failure: null },
    { label: "error", failure: new Error("rename failed") }
  ])("restores the committed model after a rename $label", async ({ failure }) => {
    const operations = createOperations();
    operations.renameMarkdownTreeFile.mockImplementation(async () => {
      if (failure) throw failure;
      return null;
    });
    const { result } = renderController([markdownTab()], operations);
    const initialResetToken = result.current.controller.modelForTab("notes-tab")?.resetToken;

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("Rejected");
      model?.onCommit("blur");
    });
    await settle();

    expect(result.current.controller.modelForTab("notes-tab")?.title).toBe("Notes");
    expect(result.current.controller.modelForTab("notes-tab")?.resetToken).toBe(
      (initialResetToken ?? 0) + 1
    );
    expect(operations.applyRenamedTreeFile).not.toHaveBeenCalled();
    expect(operations.handleMarkdownTabChange).not.toHaveBeenCalled();
    expect(operations.saveMarkdownTabContentById).not.toHaveBeenCalled();
  });

  it("reports typed invalid-title and rename-collision failures to the application boundary", async () => {
    const operations = createOperations();
    const onFailure = vi.fn();
    operations.renameMarkdownTreeFile.mockRejectedValueOnce(new Error("File already exists"));
    const { result } = renderController(
      [markdownTab()],
      operations,
      new Set(),
      undefined,
      onFailure
    );

    act(() => {
      result.current.controller.modelForTab("notes-tab")?.onInput("   ");
      result.current.controller.modelForTab("notes-tab")?.onCommit("blur");
    });
    await settle();
    act(() => {
      result.current.controller.modelForTab("notes-tab")?.onInput("Existing");
      result.current.controller.modelForTab("notes-tab")?.onCommit("blur");
    });
    await settle();

    expect(onFailure.mock.calls).toEqual([
      [{ reason: "invalid", tabId: "notes-tab" }],
      [{ reason: "rename-collision", tabId: "notes-tab" }]
    ]);
  });

  it("reports a metadata failure when immediate reconciliation cannot save", async () => {
    const operations = createOperations();
    const onFailure = vi.fn();
    const { result } = renderController(
      [markdownTab({ content: "# Body\n" })],
      operations,
      new Set(),
      undefined,
      onFailure
    );

    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });

    expect(onFailure).toHaveBeenCalledWith({
      reason: "metadata-blocked",
      tabId: "notes-tab"
    });
    expect(result.current.tabs[0]).toMatchObject({
      content: "---\ntitle: Notes\n---\n\n# Body\n",
      dirty: true
    });
    expect(operations.handleMarkdownTabChange).toHaveBeenCalledWith(
      "notes-tab",
      "---\ntitle: Notes\n---\n\n# Body\n",
      { documentRevision: 7, surface: "source" }
    );
  });

  it("distinguishes a blocked rename from an existing-name collision", async () => {
    const operations = createOperations();
    const onFailure = vi.fn();
    operations.renameMarkdownTreeFile.mockRejectedValueOnce(new Error("Permission denied"));
    const { result } = renderController(
      [markdownTab()],
      operations,
      new Set(),
      undefined,
      onFailure
    );

    act(() => {
      result.current.controller.modelForTab("notes-tab")?.onInput("Blocked");
      result.current.controller.modelForTab("notes-tab")?.onCommit("blur");
    });
    await settle();

    expect(onFailure).toHaveBeenCalledWith({
      reason: "rename-blocked",
      tabId: "notes-tab"
    });
  });

  it("visibly resets a focused failed draft and accepts a subsequent edit", async () => {
    const operations = createOperations();
    operations.renameMarkdownTreeFile
      .mockRejectedValueOnce(new Error("rename failed"))
      .mockImplementationOnce(async (file, fileName) => renamedFile(file, fileName));
    render(<ControllerTitleEditorHarness operations={operations} />);
    const editor = screen.getByRole("textbox", { name: "Document title" });

    editor.focus();
    editor.textContent = "Rejected";
    fireEvent.input(editor);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    await settle();

    expect(editor).toHaveTextContent("Notes");
    expect(editor).toHaveFocus();

    editor.textContent = "Accepted";
    fireEvent.input(editor);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    await settle();

    expect(editor).toHaveTextContent("Accepted");
    expect(operations.renameMarkdownTreeFile.mock.calls.map((call) => call[1])).toEqual([
      "Rejected.md",
      "Accepted.md"
    ]);
  });

  it.each(["collision", "error"] as const)(
    "keeps a newer focused draft when an older in-flight rename ends with %s",
    async (failure) => {
      const operations = createOperations();
      const olderRename = deferred<NativeMarkdownFolderFile | null>();
      operations.renameMarkdownTreeFile
        .mockImplementationOnce(() => olderRename.promise)
        .mockImplementationOnce(async (file, fileName) => renamedFile(file, fileName));
      render(<ControllerTitleEditorHarness operations={operations} />);
      const editor = screen.getByRole("textbox", { name: "Document title" });

      editor.focus();
      editor.textContent = "Older";
      fireEvent.input(editor);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(256);
      });

      editor.textContent = "Newer";
      fireEvent.input(editor);
      if (failure === "error") {
        olderRename.reject(new Error("older rename failed"));
      } else {
        olderRename.resolve(null);
      }
      await settle();

      expect(editor).toHaveTextContent("Newer");
      expect(editor).toHaveFocus();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(256);
      });
      await settle();

      expect(editor).toHaveTextContent("Newer");
      expect(screen.getByTestId("controller-model-title")).toHaveTextContent("Newer");
      expect(screen.getByTestId("controller-file-name")).toHaveTextContent("Newer.md");
    }
  );

  it.each(["collision", "error"] as const)(
    "syncs filename authority when a newer title-removal repair follows an older %s",
    async (failure) => {
      const operations = createOperations();
      const olderRename = deferred<NativeMarkdownFolderFile | null>();
      operations.renameMarkdownTreeFile.mockImplementationOnce(() => olderRename.promise);
      render(<ControllerTitleEditorHarness operations={operations} />);
      const editor = screen.getByRole("textbox", { name: "Document title" });

      editor.focus();
      editor.textContent = "Rejected older draft";
      fireEvent.input(editor);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(256);
      });
      fireEvent.click(screen.getByRole("button", { name: "Remove source title" }));

      if (failure === "error") {
        olderRename.reject(new Error("older rename failed"));
      } else {
        olderRename.resolve(null);
      }
      await settle();

      const repairedSource = "---\ntag: repair edit\ntitle: Notes\n---\n\nRepaired body\n";
      expect(editor).toHaveTextContent("Notes");
      expect(editor).toHaveFocus();
      expect(screen.getByTestId("controller-model-title")).toHaveTextContent("Notes");
      expect(screen.getByTestId("controller-file-name")).toHaveTextContent("Notes.md");
      expect(screen.getByTestId("controller-source").textContent).toBe(repairedSource);
      expect(operations.saveMarkdownTabContentById).toHaveBeenCalledWith(
        "notes-tab",
        repairedSource,
        { skipHistorySnapshot: true }
      );
    }
  );

  it("does not let a stale title-removal repair clobber a still-newer visual draft", async () => {
    const operations = createOperations();
    const olderRename = deferred<NativeMarkdownFolderFile | null>();
    operations.renameMarkdownTreeFile
      .mockImplementationOnce(() => olderRename.promise)
      .mockImplementationOnce(async (file, fileName) => renamedFile(file, fileName));
    render(<ControllerTitleEditorHarness operations={operations} />);
    const editor = screen.getByRole("textbox", { name: "Document title" });

    editor.focus();
    editor.textContent = "Rejected older draft";
    fireEvent.input(editor);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove source title" }));
    editor.textContent = "Newest visual";
    fireEvent.input(editor);

    olderRename.resolve(null);
    await settle();
    expect(editor).toHaveTextContent("Newest visual");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    await settle();

    expect(editor).toHaveTextContent("Newest visual");
    expect(screen.getByTestId("controller-model-title")).toHaveTextContent("Newest visual");
    expect(screen.getByTestId("controller-file-name")).toHaveTextContent("Newest visual.md");
  });

  it.each([
    {
      draft: "Unsafe/Title",
      expectedTitle: "Unsafe／Title",
      returnedName: "Unsafe／Title.md"
    },
    {
      draft: "Numbered",
      expectedTitle: "Numbered 2",
      returnedName: "Numbered 2.md"
    }
  ])("shows the authoritative $expectedTitle stem after the current rename", async ({
    draft,
    expectedTitle,
    returnedName
  }) => {
    const operations = createOperations();
    operations.renameMarkdownTreeFile.mockImplementationOnce(async (file) => (
      renamedFile(file, returnedName)
    ));
    render(<ControllerTitleEditorHarness operations={operations} />);
    const editor = screen.getByRole("textbox", { name: "Document title" });

    editor.focus();
    editor.textContent = draft;
    fireEvent.input(editor);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    await settle();

    expect(editor).toHaveTextContent(expectedTitle);
    expect(editor).toHaveFocus();
    expect(screen.getByTestId("controller-model-title")).toHaveTextContent(expectedTitle);
    expect(screen.getByTestId("controller-file-name")).toHaveTextContent(returnedName);
  });

  it("renames only the latest of two rapid visual edits", async () => {
    const { result, operations } = renderController([markdownTab()]);
    const model = result.current.controller.modelForTab("notes-tab");

    act(() => model?.onInput("First"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(128);
    });
    act(() => model?.onInput("Second"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    await settle();

    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledTimes(1);
    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledWith(expect.any(Object), "Second.md");
  });

  it("supersedes a queued draft before its I/O begins", async () => {
    const operations = createOperations();
    const firstRename = deferred<NativeMarkdownFolderFile | null>();
    operations.renameMarkdownTreeFile
      .mockImplementationOnce(() => firstRename.promise)
      .mockImplementation(async (file, fileName) => renamedFile(file, fileName));
    const { result } = renderController([markdownTab()], operations);

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("In flight");
      model?.onCommit("blur");
    });
    await settle();
    act(() => result.current.controller.modelForTab("notes-tab")?.onInput("Queued"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("Newest");
      model?.onCommit("enter");
    });

    firstRename.resolve(renamedFile({
      name: "Notes.md",
      path: "/vault/Notes.md",
      relativePath: "/vault/Notes.md"
    }, "In flight.md"));
    await settle();

    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledTimes(2);
    expect(operations.renameMarkdownTreeFile.mock.calls.map((call) => call[1])).toEqual([
      "In flight.md",
      "Newest.md"
    ]);
  });

  it("keeps late rename results scoped to their originating tabs", async () => {
    const operations = createOperations();
    const firstRename = deferred<NativeMarkdownFolderFile | null>();
    operations.renameMarkdownTreeFile.mockImplementation((file, fileName) => (
      file.path === "/vault/First.md"
        ? firstRename.promise
        : Promise.resolve(renamedFile(file, fileName))
    ));
    const { result } = renderController([
      markdownTab({ id: "first-tab", name: "First.md", path: "/vault/First.md", content: "# First\n" }),
      markdownTab({ id: "second-tab", name: "Second.md", path: "/vault/Second.md", content: "# Second\n" })
    ], operations);

    act(() => {
      const model = result.current.controller.modelForTab("first-tab");
      model?.onInput("First changed");
      model?.onCommit("blur");
    });
    await settle();
    act(() => {
      const model = result.current.controller.modelForTab("second-tab");
      model?.onInput("Second changed");
      model?.onCommit("blur");
    });
    await settle();

    expect(result.current.controller.modelForTab("second-tab")?.title).toBe("Second changed");
    firstRename.resolve({
      name: "First changed.md",
      path: "/vault/First changed.md",
      relativePath: "/vault/First changed.md"
    });
    await settle();

    expect(result.current.controller.modelForTab("first-tab")?.title).toBe("First changed");
    expect(result.current.controller.modelForTab("second-tab")?.title).toBe("Second changed");
    expect(result.current.tabs.find((tab) => tab.id === "first-tab")?.content).toContain("title: First changed");
    expect(result.current.tabs.find((tab) => tab.id === "second-tab")?.content).toContain("title: Second changed");
  });

  it("turns a valid source title edit into the same authoritative rename transaction", async () => {
    const operations = createOperations();
    operations.renameMarkdownTreeFile.mockImplementation(async (file) => renamedFile(file, "Source title 2.md"));
    operations.saveMarkdownTabContentById.mockResolvedValue({
      name: "Source title 2.md",
      path: "/vault/Source title 2.md"
    });
    const previousSource = "---\ntitle: Notes\ntag: keep\n---\n\nOld body\n";
    const nextSource = "---\ntitle: Source title\ntag: keep\n---\n\nChanged body\n";
    const { result } = renderController([markdownTab({ content: previousSource })], operations);

    let consumed = false;
    act(() => {
      consumed = result.current.controller.handleSourceTitleChange(
        "notes-tab",
        previousSource,
        nextSource
      );
    });
    expect(consumed).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    await settle();

    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledWith(
      expect.any(Object),
      "Source title.md"
    );
    expect(result.current.tabs[0]?.content).toBe(
      "---\ntitle: Source title 2\ntag: keep\n---\n\nChanged body\n"
    );
  });

  it("preserves a queued consumed source edit after an in-flight transaction authors its title", async () => {
    const operations = createOperations();
    const firstRename = deferred<NativeMarkdownFolderFile | null>();
    operations.renameMarkdownTreeFile
      .mockImplementationOnce(() => firstRename.promise)
      .mockImplementationOnce(async (file) => renamedFile(file, "Final source 2.md"));
    const initialSource = "---\ntitle: Notes\ntag: original\n---\n\nOriginal body\n";
    const sourceEdit = "---\ntitle: Final source\ntag: source edit\n---\n\nNewer source body\n";
    const { result } = renderController([markdownTab({ content: initialSource })], operations);

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("First visual");
      model?.onCommit("blur");
    });
    await settle();

    let consumed = false;
    act(() => {
      consumed = result.current.controller.handleSourceTitleChange(
        "notes-tab",
        initialSource,
        sourceEdit
      );
    });
    expect(consumed).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });

    firstRename.resolve({
      name: "First visual.md",
      path: "/vault/First visual.md",
      relativePath: "/vault/First visual.md"
    });
    await settle();

    expect(result.current.tabs[0]?.content).toBe(
      "---\ntitle: Final source 2\ntag: source edit\n---\n\nNewer source body\n"
    );
    expect(operations.renameMarkdownTreeFile.mock.calls.map((call) => call[1])).toEqual([
      "First visual.md",
      "Final source.md"
    ]);
  });

  it("preserves a queued title removal after an in-flight transaction authors its title", async () => {
    const operations = createOperations();
    const firstRename = deferred<NativeMarkdownFolderFile | null>();
    operations.renameMarkdownTreeFile.mockImplementationOnce(() => firstRename.promise);
    const initialSource = "---\ntitle: Notes\ntag: original\n---\n\nOriginal body\n";
    const removedTitleSource = "---\ntag: removal edit\n---\n\nNewer removal body\n";
    const { result } = renderController([markdownTab({ content: initialSource })], operations);

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("First visual");
      model?.onCommit("blur");
    });
    await settle();
    expect(result.current.controller.handleSourceTitleChange(
      "notes-tab",
      initialSource,
      removedTitleSource
    )).toBe(true);

    firstRename.resolve({
      name: "First visual.md",
      path: "/vault/First visual.md",
      relativePath: "/vault/First visual.md"
    });
    await settle();

    expect(result.current.tabs[0]?.content).toBe(
      "---\ntag: removal edit\ntitle: First visual\n---\n\nNewer removal body\n"
    );
    expect(operations.renameMarkdownTreeFile).toHaveBeenCalledTimes(1);
  });

  it("keeps a later ordinary source edit ahead of an older consumed source request", async () => {
    const operations = createOperations();
    const firstRename = deferred<NativeMarkdownFolderFile | null>();
    operations.renameMarkdownTreeFile
      .mockImplementationOnce(() => firstRename.promise)
      .mockImplementationOnce(async (file) => renamedFile(file, "Final source.md"));
    const initialSource = "---\ntitle: Notes\ntag: original\n---\n\nOriginal body\n";
    const consumedSource = "---\ntitle: Final source\nrequest: keep\ntag: source edit\n---\n\nSource body\n";
    const laterSource = "---\ntitle: Final source\nrequest: keep\ntag: later edit\n---\n\nLatest body\n";
    const { result } = renderController([markdownTab({ content: initialSource })], operations);

    act(() => {
      const model = result.current.controller.modelForTab("notes-tab");
      model?.onInput("First visual");
      model?.onCommit("blur");
    });
    await settle();
    expect(result.current.controller.handleSourceTitleChange(
      "notes-tab",
      initialSource,
      consumedSource
    )).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(256);
    });
    act(() => {
      result.current.setTabs((tabs) => tabs.map((tab) => tab.id === "notes-tab"
        ? { ...tab, content: laterSource, dirty: true }
        : tab));
    });

    firstRename.resolve({
      name: "First visual.md",
      path: "/vault/First visual.md",
      relativePath: "/vault/First visual.md"
    });
    await settle();

    expect(result.current.tabs[0]?.content).toBe(
      "---\ntitle: Final source\nrequest: keep\ntag: later edit\n---\n\nLatest body\n"
    );
  });

  it("leaves source body and unrelated metadata edits on the normal routing path", () => {
    const operations = createOperations();
    const previousSource = "---\ntitle: Notes\ntag: one\n---\n\nOld body\n";
    const nextSource = "---\ntitle: Notes\ntag: two\n---\n\nChanged body\n";
    const { result } = renderController([markdownTab({ content: previousSource })], operations);

    const consumed = result.current.controller.handleSourceTitleChange(
      "notes-tab",
      previousSource,
      nextSource
    );

    expect(consumed).toBe(false);
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
    expect(operations.handleMarkdownTabChange).not.toHaveBeenCalled();
    expect(operations.saveMarkdownTabContentById).not.toHaveBeenCalled();
  });

  it.each([
    "---\ntitle: [broken\n---\n",
    "+++\ntitle = [broken\n+++\n",
    "{\n  \"title\": \"broken\",\n}\n"
  ])("does nothing while source Front Matter is temporarily malformed", (nextSource) => {
    const operations = createOperations();
    const previousSource = "---\ntitle: Notes\n---\n\nBody\n";
    const { result } = renderController([markdownTab({ content: previousSource })], operations);

    const consumed = result.current.controller.handleSourceTitleChange(
      "notes-tab",
      previousSource,
      nextSource
    );

    expect(consumed).toBe(false);
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
    expect(operations.handleMarkdownTabChange).not.toHaveBeenCalled();
    expect(operations.saveMarkdownTabContentById).not.toHaveBeenCalled();
  });

  it("repairs a removed source title immediately without requesting an empty rename", async () => {
    const operations = createOperations();
    const previousSource = "---\ntitle: Notes\ntag: keep\n---\n\nBody\n";
    const nextSource = "---\ntag: keep\n---\n\nBody changed\n";
    const { result } = renderController([markdownTab({ content: previousSource })], operations);

    let consumed = false;
    await act(async () => {
      consumed = result.current.controller.handleSourceTitleChange(
        "notes-tab",
        previousSource,
        nextSource
      );
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(consumed).toBe(true);
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
    expect(result.current.tabs[0]?.content).toBe(
      "---\ntag: keep\ntitle: Notes\n---\n\nBody changed\n"
    );
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledTimes(1);
  });

  it("consumes an echoed controller-authored source patch without starting a loop", async () => {
    const operations = createOperations();
    const initialSource = "# Body\n";
    let immediateConsumed = false;
    let rendered: ReturnType<typeof renderController> | null = null;
    rendered = renderController(
      [markdownTab({ content: initialSource })],
      operations,
      new Set(),
      (tabId, source) => {
        immediateConsumed = rendered?.result.current.controller.handleSourceTitleChange(
          tabId,
          initialSource,
          source
        ) ?? false;
      }
    );

    await act(async () => {
      await rendered?.result.current.controller.reconcileOpenDocument("notes-tab");
    });

    expect(immediateConsumed).toBe(true);
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledTimes(1);
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
  });

  it("does not consume a later genuine undo to an already-observed authored source", async () => {
    const operations = createOperations();
    const initialSource = "# Body\n";
    const { result } = renderController([markdownTab({ content: initialSource })], operations);

    await act(async () => {
      await result.current.controller.reconcileOpenDocument("notes-tab");
    });
    const authoredSource = result.current.tabs[0]?.content ?? "";
    const laterBodyEdit = "---\ntitle: Notes\n---\n\nLater body edit\n";
    act(() => {
      result.current.setTabs((tabs) => tabs.map((tab) => tab.id === "notes-tab"
        ? { ...tab, content: laterBodyEdit, revision: 8 }
        : tab));
    });

    const consumed = result.current.controller.handleSourceTitleChange(
      "notes-tab",
      laterBodyEdit,
      authoredSource
    );

    expect(consumed).toBe(false);
    expect(operations.saveMarkdownTabContentById).toHaveBeenCalledTimes(1);
    expect(operations.renameMarkdownTreeFile).not.toHaveBeenCalled();
  });
});
