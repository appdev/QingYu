import { fireEvent, render, screen } from "@testing-library/react";

const lazySurface = vi.hoisted(() => {
  const state = {
    suspend: true,
    loadingPromise: Promise.resolve(),
    releaseLoading: () => {},
    reset: () => {}
  };

  state.reset = () => {
    state.suspend = true;
    state.loadingPromise = new Promise<void>((resolve) => {
      state.releaseLoading = resolve;
    });
  };
  state.reset();

  return state;
});

vi.mock("./MarkdownPaperSurface", () => ({
  MarkdownPaperSurface: () => {
    if (lazySurface.suspend) throw lazySurface.loadingPromise;

    return <div aria-label="Markdown document" className="cm-content" role="textbox" tabIndex={0} />;
  }
}));

import { MarkdownPaper } from "./MarkdownPaper";

describe("MarkdownPaper lazy loading", () => {
  beforeEach(() => {
    lazySurface.reset();
  });

  it("keeps the paper shell visible while the visual editor surface loads", () => {
    const { container } = render(
      <MarkdownPaper
        initialContent=""
        onEditorReady={() => {}}
        onMarkdownChange={() => {}}
        revision={0}
      />
    );

    expect(container.querySelector('[data-editor-engine="codemirror"]')).toBeInTheDocument();
    expect(container.querySelector('[data-editor-engine="codemirror-loading"]')).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Document title" })).not.toBeInTheDocument();
  });

  it("keeps the title and editor body in the paper article and moves focus to the body after Enter", async () => {
    lazySurface.suspend = false;
    lazySurface.releaseLoading();
    const onCommit = vi.fn();
    const { container } = render(
      <MarkdownPaper
        documentTitle={{ resetToken: 0, title: "Project outline", onCommit, onInput: () => {} }}
        initialContent=""
        onEditorReady={() => {}}
        onMarkdownChange={() => {}}
        revision={0}
      />
    );

    const titleEditor = screen.getByRole("textbox", { name: "Document title" });
    const markdownBody = await screen.findByRole("textbox", { name: "Markdown document" });

    expect(titleEditor.closest("article")).toBe(markdownBody.closest("article"));

    titleEditor.focus();
    fireEvent.keyDown(titleEditor, { key: "Enter" });

    expect(onCommit).toHaveBeenCalledExactlyOnceWith("enter");
    expect(markdownBody).toHaveFocus();
    expect(container.querySelector(".paper-scroll article")?.contains(titleEditor)).toBe(true);
  });

  it("leaves Enter alone while the title is confirming an IME candidate", async () => {
    lazySurface.suspend = false;
    lazySurface.releaseLoading();
    const onCommit = vi.fn();
    render(
      <MarkdownPaper
        documentTitle={{ resetToken: 0, title: "Project outline", onCommit, onInput: () => {} }}
        initialContent=""
        onEditorReady={() => {}}
        onMarkdownChange={() => {}}
        revision={0}
      />
    );

    const titleEditor = screen.getByRole("textbox", { name: "Document title" });
    const markdownBody = await screen.findByRole("textbox", { name: "Markdown document" });
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Enter" });
    Object.defineProperty(event, "isComposing", { value: true });

    titleEditor.focus();
    titleEditor.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    expect(onCommit).not.toHaveBeenCalled();
    expect(titleEditor).toHaveFocus();
    expect(markdownBody).not.toHaveFocus();
  });
});
