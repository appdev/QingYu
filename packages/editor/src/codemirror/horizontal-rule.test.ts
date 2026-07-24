import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { horizontalRulePlugin } from "./horizontal-rule.ts";
import { liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [horizontalRulePlugin()] })],
      selection: EditorSelection.cursor(doc.length),
    }),
  });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("horizontalRulePlugin", () => {
  it("renders a horizontal rule without changing its Markdown", () => {
    const doc = "First\n\n---\n\nSecond";
    const view = createView(doc);

    expect(view.dom.querySelector("hr.cm-markra-horizontal-rule")).not.toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("reveals the exact source only when the rendered line is activated", () => {
    const doc = "First\n\n---\n\nSecond";
    const view = createView(doc);
    const rule = view.dom.querySelector<HTMLElement>("hr.cm-markra-horizontal-rule");

    rule?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));

    expect(view.state.selection.main.head).toBe(doc.indexOf("---"));
    expect(view.dom.querySelector("hr.cm-markra-horizontal-rule")).toBeNull();
    expect(view.dom.textContent).toContain("---");
  });

  it("keeps horizontal-rule source visible while dragging from inside it", () => {
    const doc = "First\n\n---\n\nSecond";
    const anchor = doc.indexOf("---");
    const view = createView(doc);

    view.focus();
    view.dispatch({ selection: EditorSelection.cursor(anchor) });
    view.dispatch({ selection: EditorSelection.range(anchor, anchor + 2) });

    expect(view.dom.querySelector("hr.cm-markra-horizontal-rule")).toBeNull();
    expect(view.dom.textContent).toContain("---");
  });

  it("keeps four-asterisk source editable while the caret is at the line end", () => {
    const view = createView("");

    view.focus();

    for (let index = 0; index < 4; index += 1) {
      const head = view.state.selection.main.head;
      view.dispatch({
        changes: { from: head, insert: "*" },
        selection: EditorSelection.cursor(head + 1),
        userEvent: "input",
      });
    }

    expect(view.state.selection.main.head).toBe(4);
    expect(view.dom.querySelector("hr.cm-markra-horizontal-rule")).toBeNull();
    expect(view.dom.textContent).toContain("****");

    view.dispatch({ selection: EditorSelection.cursor(2) });

    view.dispatch({
      changes: { from: view.state.selection.main.head, insert: "bold" },
      selection: EditorSelection.cursor(6),
      userEvent: "input",
    });

    expect(view.state.doc.toString()).toBe("**bold**");
  });
});
