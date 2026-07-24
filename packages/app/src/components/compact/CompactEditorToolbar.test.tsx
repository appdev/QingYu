import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { vi } from "vitest";
import type { SelectionFormattingState } from "../../lib/selection-formatting";
import { CompactEditorToolbar } from "./CompactEditorToolbar";

function editorController() {
  return {
    getSelectionFormattingState: vi.fn<() => SelectionFormattingState>(() => ({
      actions: ["bold", "bulletList"],
      headingLevel: 2
    })),
    importLocalImages: vi.fn(),
    insertMarkdownImage: vi.fn(),
    insertMarkdownLink: vi.fn(),
    runFormattingAction: vi.fn(() => true),
    runEditorShortcut: vi.fn(() => true),
    setSelectionHeadingLevel: vi.fn(() => true),
    toggleTaskList: vi.fn(() => true)
  };
}

function toolbarProps(
  editor = editorController(),
  overrides: Partial<Omit<ComponentProps<typeof CompactEditorToolbar>, "editor" | "language">> = {}
) {
  return {
    editor,
    language: "en" as const,
    ...overrides
  };
}

describe("CompactEditorToolbar", () => {
  it.each([
    ["Undo", "shortcut", ["z"]],
    ["Redo", "shortcut", ["z", { shiftKey: true }]],
    ["Paragraph", "formatting", ["paragraph"]],
    ["Heading 1", "heading", [1]],
    ["Heading 2", "heading", [2]],
    ["Heading 3", "heading", [3]],
    ["Bold", "formatting", ["bold"]],
    ["Italic", "formatting", ["italic"]],
    ["Strikethrough", "formatting", ["strikethrough"]],
    ["Inline code", "formatting", ["inlineCode"]],
    ["Link", "link", []],
    ["Bullet list", "formatting", ["bulletList"]],
    ["Ordered list", "formatting", ["orderedList"]],
    ["Task list", "taskList", []],
    ["Quote", "formatting", ["quote"]],
    ["Code block", "formatting", ["codeBlock"]],
    ["Image", "image", []]
  ] as const)("delegates %s exactly once", (label, action, args) => {
    const editor = editorController();
    render(<CompactEditorToolbar {...toolbarProps(editor)} />);

    const button = screen.getByRole("button", { name: label });
    expect(fireEvent.pointerDown(button)).toBe(false);
    fireEvent.click(button);

    const target = action === "shortcut"
      ? editor.runEditorShortcut
      : action === "formatting"
        ? editor.runFormattingAction
      : action === "heading"
        ? editor.setSelectionHeadingLevel
        : action === "link"
          ? editor.insertMarkdownLink
          : action === "taskList"
            ? editor.toggleTaskList
            : editor.insertMarkdownImage;
    expect(target).toHaveBeenCalledTimes(1);
    expect(target).toHaveBeenCalledWith(...args);
  });

  it("reflects current formatting and heading state", () => {
    render(<CompactEditorToolbar {...toolbarProps()} />);

    expect(screen.getByRole("button", { name: "Bold" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Bullet list" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Heading 2" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Heading 1" })).toHaveAttribute("aria-pressed", "false");
  });

  it("uses local image import for the true-mobile image action", () => {
    const editor = editorController();
    render(<CompactEditorToolbar {...toolbarProps(editor, {
      imageImport: true,
      trueMobile: true
    })} />);

    fireEvent.click(screen.getByRole("button", { name: "Image" }));

    expect(editor.importLocalImages).toHaveBeenCalledTimes(1);
    expect(editor.insertMarkdownImage).not.toHaveBeenCalled();
  });

  it("keeps the image skeleton action in compact desktop simulation", () => {
    const editor = editorController();
    render(<CompactEditorToolbar {...toolbarProps(editor, {
      imageImport: true,
      trueMobile: false
    })} />);

    fireEvent.click(screen.getByRole("button", { name: "Image" }));

    expect(editor.insertMarkdownImage).toHaveBeenCalledTimes(1);
    expect(editor.importLocalImages).not.toHaveBeenCalled();
  });

  it("disables mobile image import when the runtime capability is unavailable", () => {
    const editor = editorController();
    render(<CompactEditorToolbar {...toolbarProps(editor, {
      imageImport: false,
      trueMobile: true
    })} />);

    const image = screen.getByRole("button", { name: "Image" });
    expect(image).toBeDisabled();
    fireEvent.click(image);
    expect(editor.importLocalImages).not.toHaveBeenCalled();
    expect(editor.insertMarkdownImage).not.toHaveBeenCalled();
  });

  it("distinguishes an active task list from a normal bullet list", () => {
    const editor = editorController();
    editor.getSelectionFormattingState.mockReturnValue({
      actions: ["taskList"],
      headingLevel: null
    });
    render(<CompactEditorToolbar {...toolbarProps(editor)} />);

    expect(screen.getByRole("button", { name: "Task list" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Bullet list" })).toHaveAttribute("aria-pressed", "false");
  });

  it("exposes disabled actions without dispatching them", () => {
    const editor = editorController();
    render(<CompactEditorToolbar {...toolbarProps(editor, { disabled: true })} />);

    const bold = screen.getByRole("button", { name: "Bold" });
    expect(bold).toHaveAttribute("aria-disabled", "true");
    fireEvent.pointerDown(bold);
    fireEvent.click(bold);
    expect(editor.runEditorShortcut).not.toHaveBeenCalled();
    expect(editor.runFormattingAction).not.toHaveBeenCalled();
  });

  it("dismisses the keyboard by blurring the active editable without dispatching an edit", () => {
    const editor = editorController();
    const editable = document.createElement("div");
    editable.setAttribute("contenteditable", "true");
    editable.tabIndex = 0;
    document.body.append(editable);
    editable.focus();
    render(<CompactEditorToolbar {...toolbarProps(editor)} />);

    const dismiss = screen.getByRole("button", { name: "Dismiss keyboard" });
    fireEvent.pointerDown(dismiss);
    fireEvent.click(dismiss);

    expect(document.activeElement).not.toBe(editable);
    expect(editor.runEditorShortcut).not.toHaveBeenCalled();
    expect(editor.runFormattingAction).not.toHaveBeenCalled();
    expect(editor.toggleTaskList).not.toHaveBeenCalled();
    editable.remove();
  });

  it("uses a horizontal non-shrinking row with 44px minimum targets", () => {
    render(<CompactEditorToolbar {...toolbarProps()} />);

    const toolbar = screen.getByRole("toolbar", { name: "Formatting" });
    expect(toolbar).toHaveClass("overflow-x-auto");
    expect(toolbar).toHaveAttribute("data-compact-scroll", "horizontal");
    expect(toolbar.getAttribute("style")).toContain(
      "bottom: var(--compact-bottom-inset)"
    );
    for (const button of screen.getAllByRole("button")) {
      expect(button).toHaveClass("min-h-11", "min-w-11", "shrink-0");
      expect(button).toHaveClass("focus-visible:outline-2", "active:bg-(--bg-active)");
    }
  });
});
