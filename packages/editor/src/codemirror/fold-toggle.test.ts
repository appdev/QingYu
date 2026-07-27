import { foldedRanges } from "@codemirror/language";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { foldTogglePlugin } from "./fold-toggle.ts";
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
      extensions: [liveMarkdown({ plugins: [foldTogglePlugin()] })],
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

describe("foldTogglePlugin", () => {
  it.each([
    [
      "heading",
      "# Synthetic heading\n\nBody",
      ".markra-heading-toggle-button",
    ],
    [
      "list",
      "- Parent item\n  - Nested item\n\nBody",
      ".markra-list-toggle-button",
    ],
  ])(
    "keeps the %s toggle outside the source-side selection boundary",
    (_kind, doc, selector) => {
      const view = createView(doc);
      view.dispatch({ selection: { anchor: 5 } });
      const toggle = view.dom.querySelector<HTMLButtonElement>(
        selector,
      );
      if (!toggle) throw new Error("Expected the fold toggle");

      // CodeMirror inserts this buffer between widgets associated before the
      // position and the Markdown source associated after it.
      expect(
        toggle.nextElementSibling?.classList.contains("cm-widgetBuffer"),
      ).toBe(true);
    },
  );

  it("collapses and expands a heading section without changing Markdown", () => {
    const doc = "# One\n\nAlpha\n\n## Child\n\nBeta\n\n# Two\n\nGamma";
    const view = createView(doc);
    const collapse = view.dom.querySelector<HTMLButtonElement>(
      '.markra-heading-toggle-button[aria-label="Collapse section"]',
    );

    collapse?.click();

    expect(foldedRanges(view.state).size).toBe(1);
    expect(view.dom.querySelector(
      '.markra-heading-toggle-button[aria-label="Expand section"]',
    )).not.toBeNull();
    expect(view.state.doc.toString()).toBe(doc);

    view.dom.querySelector<HTMLButtonElement>(
      '.markra-heading-toggle-button[aria-label="Expand section"]',
    )?.click();
    expect(foldedRanges(view.state).size).toBe(0);
  });

  it("collapses only the nested content of a parent list item", () => {
    const doc = "- Parent\n  - Child one\n  - Child two\n- After";
    const view = createView(doc);
    const collapse = view.dom.querySelector<HTMLButtonElement>(
      '.markra-list-toggle-button[aria-label="Collapse list item"]',
    );

    collapse?.click();

    expect(foldedRanges(view.state).size).toBe(1);
    expect(view.dom.querySelector(
      '.markra-list-toggle-button[aria-label="Expand list item"]',
    )).not.toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });
});
