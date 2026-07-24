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

    const header = view.dom.querySelector<HTMLElement>(".markra-callout-header");
    const select = view.dom.querySelector<HTMLElement>(".markra-callout-type-select");
    expect(header?.textContent).toContain("Warning");
    expect(header && getComputedStyle(header).display).toBe("inline-flex");
    expect(header && getComputedStyle(header).width).not.toBe("100%");
    expect(select && getComputedStyle(select).position).toBe("absolute");
    expect(calloutLines).toHaveLength(3);
    expect(calloutLines[0]?.getAttribute("data-callout-type")).toBe("warning");
    expect(calloutLines[0]?.classList.contains("markra-callout-first")).toBe(true);
    expect(calloutLines[0]?.classList.contains("markra-callout-last")).toBe(false);
    expect(calloutLines[1]?.classList.contains("markra-callout-first")).toBe(false);
    expect(calloutLines[1]?.classList.contains("markra-callout-last")).toBe(false);
    expect(calloutLines[2]?.classList.contains("markra-callout-first")).toBe(false);
    expect(calloutLines[2]?.classList.contains("markra-callout-last")).toBe(true);
    expect(
      Array.from(calloutLines).some((line) =>
        line.classList.contains("markra-callout-active"),
      ),
    ).toBe(false);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("marks every visual row active while the caret is inside the callout", () => {
    const doc = "> [!NOTE]\n>\n> Synthetic detail\n\nEdit";
    const view = createView(doc);

    view.dispatch({
      selection: EditorSelection.cursor(doc.indexOf("Synthetic") + 4),
    });

    const calloutLines = view.dom.querySelectorAll(".cm-markra-callout");
    expect(calloutLines).toHaveLength(3);
    expect(
      Array.from(calloutLines).every((line) =>
        line.classList.contains("markra-callout-active"),
      ),
    ).toBe(true);
  });

  it("keeps the callout marker source visible while dragging from inside it", () => {
    const doc = "> [!NOTE]\n>\n> Synthetic detail\n\nEdit";
    const anchor = doc.indexOf("[!NOTE]") + 2;
    const view = createView(doc);

    view.dispatch({ selection: EditorSelection.cursor(anchor) });
    expect(view.dom.querySelector(".markra-callout-header")).toBeNull();

    view.dispatch({ selection: EditorSelection.range(anchor, anchor + 4) });

    expect(view.dom.querySelector(".markra-callout-header")).toBeNull();
    expect(view.dom.textContent).toContain("[!NOTE]");
  });

  it("keeps the callout layout stable during a multi-line range selection", () => {
    const doc = "> [!NOTE]\n>\n> Synthetic detail\n\nEdit";
    const view = createView(doc);

    view.dispatch({ selection: EditorSelection.range(0, doc.length) });

    expect(view.dom.querySelector(".markra-callout-header")?.textContent).toContain("Note");
    expect(
      Array.from(view.dom.querySelectorAll(".cm-markra-callout")).some((line) =>
        line.classList.contains("markra-callout-active"),
      ),
    ).toBe(false);
  });

  it("uses measured block spacers instead of line margins around a callout", () => {
    const view = createView("Before\n\n> [!NOTE]\n> Synthetic detail\n\nAfter");
    const spacers = view.dom.querySelectorAll(".markra-callout-spacer");

    expect(spacers).toHaveLength(2);
    expect(
      spacers[0]?.classList.contains("markra-callout-spacer-before"),
    ).toBe(true);
    expect(
      spacers[1]?.classList.contains("markra-callout-spacer-after"),
    ).toBe(true);
    expect(
      Number.parseFloat(
        getComputedStyle(
          view.dom.querySelector<HTMLElement>(".markra-callout-first")!,
        ).marginTop,
      ),
    ).toBe(0);
    expect(
      Number.parseFloat(
        getComputedStyle(
          view.dom.querySelector<HTMLElement>(".markra-callout-last")!,
        ).marginBottom,
      ),
    ).toBe(0);
  });

  it("marks a one-line callout as both the first and last visual row", () => {
    const view = createView("> [!NOTE]");
    const callout = view.dom.querySelector(".cm-markra-callout");

    expect(callout?.classList.contains("markra-callout-first")).toBe(true);
    expect(callout?.classList.contains("markra-callout-last")).toBe(true);
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
