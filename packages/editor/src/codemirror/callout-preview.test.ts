import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { calloutPreviewPlugin, liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string, enabled = true) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [calloutPreviewPlugin({ enabled })] })],
      selection: EditorSelection.cursor(doc.length),
    }),
  });
  view.focus();
  view.dispatch({ selection: view.state.selection });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("calloutPreviewPlugin", () => {
  it("renders a GitHub alert as a callout without changing Markdown", () => {
    const doc = "> [!WARNING]\n>\n> Synthetic detail\n\nEdit";
    const view = createView(doc);
    const calloutLines = view.dom.querySelectorAll(".cm-markra-callout");

    expect(view.dom.querySelector(".markra-callout-header")?.textContent).toContain("Warning");
    expect(calloutLines).toHaveLength(3);
    expect(calloutLines[0]?.getAttribute("data-callout-type")).toBe("warning");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("changes the callout type through its header control", () => {
    const doc = "> [!NOTE]\n> Synthetic detail\n\nEdit";
    const view = createView(doc);
    const select = view.dom.querySelector<HTMLSelectElement>(".markra-callout-type-select");

    expect(select).not.toBeNull();
    if (select) {
      select.value = "caution";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }

    expect(view.state.doc.toString()).toBe("> [!CAUTION]\n> Synthetic detail\n\nEdit");
    expect(view.dom.querySelector(".markra-callout-header")?.textContent).toContain("Caution");
  });

  it("respects the GitHub alert preference gate", () => {
    const doc = "> [!TIP]\n> Synthetic detail\n\nEdit";
    const view = createView(doc, false);

    expect(view.dom.querySelector(".markra-callout-header")).toBeNull();
    expect(view.dom.querySelector(".cm-markra-callout")).toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });
});
