import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { DocumentTitleEditor } from "./DocumentTitleEditor";

function renderEditor(overrides: Partial<ComponentProps<typeof DocumentTitleEditor>> = {}) {
  const onCommit = vi.fn();
  const onInput = vi.fn();

  render(
    <DocumentTitleEditor
      language="en"
      resetToken={0}
      title="A complete document title"
      onCommit={onCommit}
      onInput={onInput}
      {...overrides}
    />
  );

  return { onCommit, onInput, titleEditor: screen.getByRole("textbox", { name: "Document title" }) };
}

describe("DocumentTitleEditor", () => {
  it("shows the complete title in a borderless, softly wrapping textbox", () => {
    const title = "A title that keeps flowing naturally when it extends past the available paper width";
    const { titleEditor } = renderEditor({ title });

    expect(titleEditor).toHaveTextContent(title);
    expect(titleEditor).toHaveAttribute("contenteditable", "true");
    expect(titleEditor).toHaveAttribute("aria-multiline", "false");
    expect(titleEditor).toHaveClass("whitespace-pre-wrap", "break-words");
    titleEditor.focus();
    expect(titleEditor).toHaveFocus();
    expect(titleEditor).toHaveClass("outline-none", "focus:outline-none");
    expect(titleEditor).not.toHaveClass("border", "ring", "focus:ring");
    expect(titleEditor).not.toHaveClass("truncate", "whitespace-nowrap", "overflow-hidden");
  });

  it("emits emoji input unchanged", () => {
    const { onInput, titleEditor } = renderEditor();

    titleEditor.textContent = "A launch plan 🚀";
    fireEvent.input(titleEditor);

    expect(onInput).toHaveBeenCalledWith("A launch plan 🚀");
  });

  it("normalizes hard line breaks in focused input to one logical line", () => {
    const { onInput, titleEditor } = renderEditor();

    titleEditor.focus();
    titleEditor.textContent = "First line\nSecond line";
    fireEvent.input(titleEditor);

    expect(onInput).toHaveBeenCalledWith("First line Second line");
    expect(titleEditor).toHaveTextContent("First line Second line");
    expect(titleEditor.textContent).not.toContain("\n");
    expect(window.getSelection()?.isCollapsed).toBe(true);
    expect(window.getSelection()?.anchorOffset).toBe("First line Second line".length);

    fireEvent.blur(titleEditor);

    expect(titleEditor).toHaveTextContent("First line Second line");
  });

  it("waits for IME composition to finish before reporting input", () => {
    const { onInput, titleEditor } = renderEditor();

    fireEvent.compositionStart(titleEditor);
    titleEditor.textContent = "输入中";
    fireEvent.input(titleEditor);
    expect(onInput).not.toHaveBeenCalled();

    fireEvent.compositionEnd(titleEditor);

    expect(onInput).toHaveBeenCalledTimes(1);
    expect(onInput).toHaveBeenCalledWith("输入中");
  });

  it("does not publish the same IME value again when the final input event arrives", () => {
    const { onInput, titleEditor } = renderEditor();

    fireEvent.compositionStart(titleEditor);
    titleEditor.textContent = "完成";
    fireEvent.input(titleEditor);
    fireEvent.compositionEnd(titleEditor);
    fireEvent.input(titleEditor);
    titleEditor.textContent = "完成了";
    fireEvent.input(titleEditor);

    expect(onInput).toHaveBeenCalledTimes(2);
    expect(onInput).toHaveBeenNthCalledWith(1, "完成");
    expect(onInput).toHaveBeenNthCalledWith(2, "完成了");
  });

  it("commits on Enter without inserting a hard line break", () => {
    const { onCommit, titleEditor } = renderEditor();

    fireEvent.keyDown(titleEditor, { key: "Enter" });

    expect(onCommit).toHaveBeenCalledWith("enter");
    expect(titleEditor).toHaveTextContent("A complete document title");
  });

  it("commits on blur", () => {
    const { onCommit, titleEditor } = renderEditor();

    fireEvent.blur(titleEditor);

    expect(onCommit).toHaveBeenCalledWith("blur");
  });

  it("leaves Escape untouched", () => {
    const { onCommit, titleEditor } = renderEditor();
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Escape" });

    titleEditor.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("is not editable when disabled", () => {
    const { titleEditor } = renderEditor({ disabled: true });

    expect(titleEditor).toHaveAttribute("contenteditable", "false");
  });

  it("does not replace text while the title is actively being edited", () => {
    const { rerender } = render(
      <DocumentTitleEditor language="en" resetToken={0} title="Original" onCommit={() => {}} onInput={() => {}} />
    );
    const titleEditor = screen.getByRole("textbox", { name: "Document title" });

    titleEditor.focus();
    titleEditor.textContent = "Still typing";
    rerender(
      <DocumentTitleEditor language="en" resetToken={0} title="Updated elsewhere" onCommit={() => {}} onInput={() => {}} />
    );

    expect(titleEditor).toHaveTextContent("Still typing");
  });

  it("force-syncs the committed title when the reset token changes while focused", () => {
    const { rerender } = render(
      <DocumentTitleEditor
        language="en"
        resetToken={0}
        title="Committed"
        onCommit={() => {}}
        onInput={() => {}}
      />
    );
    const titleEditor = screen.getByRole("textbox", { name: "Document title" });

    titleEditor.focus();
    titleEditor.textContent = "Rejected draft";
    rerender(
      <DocumentTitleEditor
        language="en"
        resetToken={1}
        title="Committed"
        onCommit={() => {}}
        onInput={() => {}}
      />
    );

    expect(titleEditor).toHaveTextContent("Committed");
    expect(titleEditor).toHaveFocus();
  });
});
