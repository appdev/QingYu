import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { footnotePreviewPlugin, liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string, anchor = doc.length) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [footnotePreviewPlugin()] })],
      selection: EditorSelection.cursor(anchor),
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

describe("footnotePreviewPlugin", () => {
  it("renders references and definition labels without changing Markdown", () => {
    const doc = "Alpha[^one]\n\n[^one]: Synthetic detail.\n\nEdit";
    const view = createView(doc);

    expect(view.dom.querySelector(".cm-markra-footnote-reference")?.textContent).toContain("one");
    expect(view.dom.querySelector(".cm-markra-footnote-definition")?.textContent).toContain("Synthetic detail.");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("shows a definition preview and navigates to its source with modifier-click", () => {
    const doc = "Alpha[^one]\n\n[^one]: Synthetic detail.\n    Continued detail.\n\nEdit";
    const view = createView(doc);
    const reference = view.dom.querySelector<HTMLElement>(".cm-markra-footnote-reference");

    reference?.dispatchEvent(new MouseEvent("mouseenter"));
    expect(view.dom.querySelector(".markra-footnote-preview")?.textContent).toContain(
      "Synthetic detail. Continued detail.",
    );

    reference?.dispatchEvent(new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      metaKey: true,
    }));
    expect(view.state.selection.main.head).toBe(doc.indexOf("Synthetic detail."));
    expect(view.dom.textContent).toContain("[^one]:");
  });

  it("reveals footnote reference source on an ordinary click", () => {
    const doc = "Alpha[^one]\n\n[^one]: Synthetic detail.\n\nEdit";
    const from = doc.indexOf("[^one]");
    const view = createView(doc);
    const reference = view.dom.querySelector<HTMLElement>(".cm-markra-footnote-reference");

    reference?.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      cancelable: true,
    }));

    expect(view.state.selection.main.head).toBeGreaterThan(from);
    expect(view.state.selection.main.head).toBeLessThan(from + "[^one]".length);
    expect(view.dom.querySelector(".cm-markra-footnote-reference")).toBeNull();
    expect(view.dom.textContent).toContain("Alpha[^one]");
  });

  it("reveals a footnote definition marker when its visual label is clicked", () => {
    const doc = "Alpha[^one]\n\n[^one]: Synthetic detail.\n\nEdit";
    const definitionFrom = doc.lastIndexOf("[^one]");
    const view = createView(doc);
    const label = view.dom.querySelector<HTMLElement>(
      ".cm-markra-footnote-definition-label",
    );

    label?.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      cancelable: true,
    }));

    expect(view.state.selection.main.head).toBeGreaterThan(definitionFrom);
    expect(view.dom.querySelector(".cm-markra-footnote-definition-label")).toBeNull();
    expect(view.dom.textContent).toContain("[^one]: Synthetic detail.");
  });

  it("reveals an editable reference when the selection enters it", () => {
    const doc = "Alpha[^one]\n\n[^one]: Synthetic detail.\n\nEdit";
    const from = doc.indexOf("[^one]");
    const view = createView(doc);

    view.dispatch({ selection: EditorSelection.cursor(from + 2) });

    expect(view.dom.querySelector(".cm-markra-footnote-reference")).toBeNull();
    expect(view.dom.textContent).toContain("Alpha[^one]");
  });

  it("keeps footnotes rendered during a multi-line range selection", () => {
    const doc = "Alpha[^one]\n\n[^one]: Synthetic detail.\n\nEdit";
    const view = createView(doc);

    view.dispatch({ selection: EditorSelection.range(0, doc.length) });

    expect(view.dom.querySelector(".cm-markra-footnote-reference")).not.toBeNull();
    expect(view.dom.querySelector(".cm-markra-footnote-definition")).not.toBeNull();
  });
});
