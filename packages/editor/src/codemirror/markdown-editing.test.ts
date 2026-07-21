import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { liveMarkdown } from "./index.ts";
import { markdownEditingPlugin } from "./markdown-editing.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string, position: number) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [markdownEditingPlugin()] })],
      selection: EditorSelection.cursor(position),
    }),
  });
  views.push(view);
  return view;
}

function press(
  view: EditorView,
  key: string,
  options: KeyboardEventInit = {},
) {
  return runScopeHandlers(
    view,
    new KeyboardEvent("keydown", { bubbles: true, key, ...options }),
    "editor",
  );
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("markdownEditingPlugin", () => {
  it("keeps CodeMirror's native Markdown list continuation", () => {
    const view = createView("- First", "- First".length);

    expect(press(view, "Enter")).toBe(true);
    expect(view.state.doc.toString()).toBe("- First\n- ");
  });

  it("inserts two spaces at the cursor for plain-text Tab", () => {
    const view = createView("Alphabeta", "Alpha".length);

    expect(press(view, "Tab")).toBe(true);
    expect(view.state.doc.toString()).toBe("Alpha  beta");
  });

  it("indents and outdents the current Markdown list item", () => {
    const doc = "- First\n- Second";
    const view = createView(doc, doc.indexOf("Second"));

    expect(press(view, "Tab")).toBe(true);
    expect(view.state.doc.toString()).toBe("- First\n  - Second");
    expect(press(view, "Tab", { shiftKey: true })).toBe(true);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("keeps Shift+Enter inside quote and callout source", () => {
    const doc = "> [!NOTE]\n> Quote";
    const view = createView(doc, doc.length);

    expect(press(view, "Enter", { shiftKey: true })).toBe(true);
    expect(view.state.doc.toString()).toBe("> [!NOTE]\n> Quote\n> ");
  });

  it("inserts an HTML line break inside a GFM table cell", () => {
    const doc = "| Name | Notes |\n| --- | --- |\n| Example | First line |";
    const position = doc.indexOf("First line") + "First line".length;
    const view = createView(doc, position);

    expect(press(view, "Enter", { shiftKey: true })).toBe(true);
    expect(view.state.doc.toString()).toContain("First line<br> |");
  });
});
