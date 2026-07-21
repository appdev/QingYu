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
});
