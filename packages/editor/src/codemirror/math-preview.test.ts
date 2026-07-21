import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { liveMarkdown, mathPreviewPlugin } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string, anchor = doc.length) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [mathPreviewPlugin()] })],
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

describe("mathPreviewPlugin", () => {
  it("renders dollar and Hugo math with KaTeX without changing Markdown", () => {
    const doc = [
      "Where $a^2 + b^2 = c^2$.",
      "",
      "$$",
      String.raw`\int_0^1 x^2 \, dx`,
      "$$",
      "",
      String.raw`\[ E = mc^2 \]`,
      "",
      "Edit",
    ].join("\n");
    const view = createView(doc);

    expect(view.dom.querySelectorAll(".markra-math-render-inline .katex")).toHaveLength(1);
    expect(view.dom.querySelectorAll(".markra-math-render-display .katex")).toHaveLength(2);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("keeps inline code, escaped dollars, currency, and unfinished math as source", () => {
    const doc = "Use \\$literal, $100, `$code$`, and unfinished $value.";
    const view = createView(doc);

    expect(view.dom.querySelector(".markra-math-render")).toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("reveals source when selected and activates rendered math from the widget", () => {
    const doc = "Before $x + y$ after\n\nEdit";
    const mathFrom = doc.indexOf("$x");
    const view = createView(doc);
    const widget = view.dom.querySelector<HTMLElement>(".markra-math-render-inline");

    expect(widget).not.toBeNull();
    widget?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
    expect(view.state.selection.main.head).toBeGreaterThan(mathFrom);
    expect(view.state.selection.main.head).toBeLessThan(mathFrom + "$x + y$".length);
    expect(view.dom.querySelector(".markra-math-render-inline")).toBeNull();
    expect(view.dom.textContent).toContain("$x + y$");
  });

  it("applies macro definitions to later formulas while folding definition-only blocks", () => {
    const doc = [
      "$$",
      String.raw`\newcommand{\RR}{\mathbb{R}}`,
      "$$",
      "",
      String.raw`Domain $\RR$.`,
      "",
      "Edit",
    ].join("\n");
    const view = createView(doc);

    expect(view.dom.querySelector(".markra-math-macro-fold")).not.toBeNull();
    expect(view.dom.querySelector(".markra-math-render-display")).toBeNull();
    expect(view.dom.querySelector(".markra-math-render-inline .mathbb")?.textContent).toContain("R");
    expect(view.state.doc.toString()).toBe(doc);
  });
});
