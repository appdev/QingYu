import { forceParsing } from "@codemirror/language";
import { EditorSelection, EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import { liveMarkdown } from "./index.ts";

const syntaxTreeIterations = vi.hoisted(
  (): Array<{ from: number | undefined; to: number | undefined }> => [],
);

vi.mock("@codemirror/language", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@codemirror/language")>();

  return {
    ...actual,
    syntaxTree(state: Parameters<typeof actual.syntaxTree>[0]) {
      const tree = actual.syntaxTree(state);
      return new Proxy(tree, {
        get(target, property, receiver) {
          if (property !== "iterate") {
            return Reflect.get(target, property, receiver);
          }

          return (spec: Parameters<typeof tree.iterate>[0]) => {
            syntaxTreeIterations.push({ from: spec.from, to: spec.to });
            return target.iterate(spec);
          };
        },
      });
    },
  };
});

const source = "# Project **notes**\n\nEdit this line";
import "./dom.test-support.ts";

const views: EditorView[] = [];

interface ViewOptions {
  doc?: string;
  anchor?: number;
  selection?: EditorSelection;
  focus?: boolean;
  extensions?: Extension[];
}

function createView({
  doc = source,
  anchor = doc.length,
  selection,
  focus = true,
  extensions = [liveMarkdown()],
}: ViewOptions = {}) {
  const parent = document.createElement("div");
  document.body.append(parent);

  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      selection: selection ?? { anchor },
      extensions: [
        EditorState.allowMultipleSelections.of(true),
        ...extensions,
      ],
    }),
  });

  views.push(view);
  if (focus) {
    view.focus();
    view.dispatch({ selection: view.state.selection });
  }
  return view;
}

function renderedLines(view: EditorView) {
  return Array.from(view.dom.querySelectorAll(".cm-line"), (line) =>
    line.textContent ?? "",
  );
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("liveMarkdown", () => {
  it("hides Markdown markers outside the active line", () => {
    const view = createView();

    expect(renderedLines(view)).toEqual([
      "Project notes",
      "",
      "Edit this line",
    ]);
  });

  it("reveals Markdown markers when the cursor enters their line", () => {
    const view = createView();

    view.dispatch({ selection: { anchor: 5 } });

    expect(renderedLines(view)[0]).toBe("# Project **notes**");
  });

  it("renders every line when the editor does not have focus", () => {
    const view = createView({ anchor: 5, focus: false });

    expect(renderedLines(view)[0]).toBe("Project notes");
  });

  it("reveals a link only when the selection enters that link", () => {
    const doc = "Before [label](https://example.test) after";
    const view = createView({ doc, anchor: 2 });

    expect(renderedLines(view)[0]).toBe("Before label after");

    view.dispatch({ selection: { anchor: doc.indexOf("label") + 2 } });

    expect(renderedLines(view)[0]).toBe(doc);
  });

  it("keeps a link rendered when the caret is immediately beside it", () => {
    const doc = "[label](https://example.test) after";
    const linkEnd = doc.indexOf(" after");
    const view = createView({ doc, anchor: linkEnd });

    expect(renderedLines(view)[0]).toBe("label after");
  });

  it("styles rendered links and blockquotes", () => {
    const doc = "> Read [reference](https://example.test)\n\nEdit";
    const view = createView({ doc });

    expect(renderedLines(view)[0]).toBe("Read reference");
    expect(view.dom.querySelector(".cm-markra-link")?.textContent).toBe(
      "reference",
    );
    expect(view.dom.querySelector(".cm-markra-link")?.tagName).toBe("A");
    expect(
      view.dom.querySelector(".cm-markra-link")?.getAttribute("href"),
    ).toBe("https://example.test");
    expect(view.dom.querySelector(".cm-markra-blockquote")).not.toBeNull();
  });

  it("enables and renders GFM strikethrough by default", () => {
    const doc = "Keep ~~old~~ new\n\nEdit";
    const view = createView({ doc });

    expect(renderedLines(view)[0]).toBe("Keep old new");
    expect(
      view.dom.querySelector(".cm-markra-strikethrough")?.textContent,
    ).toBe("old");
  });

  it("renders Markra highlight syntax and reveals its markers for editing", () => {
    const doc = "Active\n\nBefore ==marked== after";
    const view = createView({ doc, anchor: 0 });

    expect(view.dom.querySelector(".cm-markra-highlight")?.textContent).toBe(
      "marked",
    );
    expect(view.dom.textContent).not.toContain("==marked==");

    const from = doc.indexOf("marked");
    view.dispatch({ selection: EditorSelection.range(from, from + 6) });
    expect(view.dom.textContent).toContain("==marked==");
  });

  it("reveals every line touched by a multi-selection", () => {
    const doc = "# One\n\n# Two\n\nRest";
    const selection = EditorSelection.create([
      EditorSelection.cursor(2),
      EditorSelection.cursor(doc.indexOf("Two") + 1),
    ]);
    const view = createView({ doc, selection });

    expect(view.hasFocus).toBe(true);
    expect(view.state.selection.ranges).toHaveLength(2);
    expect(renderedLines(view)).toEqual(["# One", "", "# Two", "", "Rest"]);
  });

  it("provides the view and syntax node name to custom reveal policies", () => {
    const contexts: Array<{ view: EditorView; nodeName: string }> = [];
    const view = createView({
      extensions: [
        liveMarkdown({
          reveal(context) {
            contexts.push({
              view: context.view,
              nodeName: context.nodeName,
            });
            return false;
          },
        }),
      ],
    });

    expect(contexts.some((context) => context.view === view)).toBe(true);
    expect(contexts.some((context) => context.nodeName === "HeaderMark")).toBe(
      true,
    );
  });

  it("limits syntax-tree traversal to CodeMirror's visible ranges", () => {
    syntaxTreeIterations.splice(0);
    const doc = Array.from(
      { length: 500 },
      (_, index) => `## Synthetic heading ${index}`,
    ).join("\n\n");
    const view = createView({ doc });
    const expectedRanges = view.visibleRanges.map(({ from, to }) => ({
      from,
      to,
    }));

    expect(expectedRanges.length).toBeGreaterThan(0);
    expect(syntaxTreeIterations.slice(-expectedRanges.length)).toEqual(
      expectedRanges,
    );
  });

  it("rebuilds decorations when the background parser advances", () => {
    const doc = Array.from(
      { length: 4_000 },
      (_, index) => `## Deferred heading ${index}\n\n**value-${index}**`,
    ).join("\n\n");
    const view = createView({ doc });

    syntaxTreeIterations.splice(0);
    expect(forceParsing(view, doc.length, 1_000)).toBe(true);
    expect(syntaxTreeIterations.length).toBeGreaterThan(0);
  });

  it("does not rebuild syntax decorations during an active composition", async () => {
    const doc = "## Synthetic title\n\nCompose";
    const view = createView({ doc });
    const inputLine = view.dom.querySelectorAll<HTMLElement>(".cm-line")[2];
    if (!inputLine) throw new Error("Expected the synthetic input line");

    syntaxTreeIterations.splice(0);
    view.contentDOM.dispatchEvent(
      new CompositionEvent("compositionstart", {
        bubbles: true,
        data: "",
      }),
    );
    inputLine.append("中");
    view.contentDOM.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        data: "中",
        inputType: "insertCompositionText",
      }),
    );

    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe(`${doc}中`);
    });

    expect(view.composing).toBe(true);
    expect(syntaxTreeIterations).toHaveLength(0);

    view.contentDOM.dispatchEvent(
      new CompositionEvent("compositionend", {
        bubbles: true,
        data: "中",
      }),
    );

    await vi.waitFor(() => {
      expect(view.composing).toBe(false);
      expect(syntaxTreeIterations.length).toBeGreaterThan(0);
    });
  });

  it("renders task markers as checkboxes that update the Markdown source", () => {
    const doc = "- [ ] Ship mock release\n\nEdit";
    const view = createView({ doc });
    const checkbox = view.dom.querySelector<HTMLInputElement>(
      ".cm-markra-task-checkbox",
    );

    expect(checkbox).not.toBeNull();
    expect(checkbox?.checked).toBe(false);
    expect(renderedLines(view)[0]).toBe("Ship mock release");

    checkbox?.click();

    expect(view.state.doc.toString()).toBe("- [x] Ship mock release\n\nEdit");
    expect(
      view.dom.querySelector<HTMLInputElement>(".cm-markra-task-checkbox")
        ?.checked,
    ).toBe(true);
  });

  it("allows task checkbox rendering to be disabled", () => {
    const doc = "- [ ] Keep the marker visible\n\nEdit";
    const view = createView({
      doc,
      extensions: [liveMarkdown({ taskCheckboxes: false })],
    });

    expect(
      view.dom.querySelector(".cm-markra-task-checkbox"),
    ).toBeNull();
    expect(renderedLines(view)[0]).toBe("- [ ] Keep the marker visible");
  });

  it("adds semantic classes without changing the Markdown document", () => {
    const view = createView();

    expect(view.dom.querySelector(".cm-markra-h1")).not.toBeNull();
    expect(view.dom.querySelector(".cm-markra-h1")?.getAttribute("role")).toBe(
      "heading",
    );
    expect(
      view.dom.querySelector(".cm-markra-h1")?.getAttribute("aria-level"),
    ).toBe("1");
    expect(
      view.dom.querySelector(".cm-markra-h1")?.getAttribute("aria-label"),
    ).toBe("Project notes");
    expect(view.dom.querySelector(".cm-markra-strong")?.textContent).toBe(
      "notes",
    );
    expect(view.state.doc.toString()).toBe(source);
  });

  it("adds document-layout semantics for paragraphs, blank lines, and lists", () => {
    const doc = [
      "# Synthetic title",
      "",
      "Synthetic paragraph",
      "",
      "- Bullet item",
      "- [ ] Task item",
      "1. Ordered item",
      "",
      "Edit here",
    ].join("\n");
    const view = createView({ doc, anchor: doc.length });
    const lines = [...view.dom.querySelectorAll<HTMLElement>(".cm-line")];

    expect(lines[1]?.classList.contains("cm-markra-empty-line")).toBe(true);
    expect(lines[2]?.classList.contains("cm-markra-paragraph")).toBe(true);
    expect(lines[4]?.classList.contains("cm-markra-list-item")).toBe(true);
    expect(lines[4]?.getAttribute("data-list-kind")).toBe("bullet");
    expect(lines[4]?.getAttribute("data-list-marker")).toBe("•");
    expect(lines[4]?.getAttribute("data-markra-list-source")).toBe("hidden");
    expect(lines[4]?.textContent).toBe("Bullet item");
    expect(lines[5]?.getAttribute("data-list-kind")).toBe("task");
    expect(lines[5]?.textContent).toBe("Task item");
    expect(lines[6]?.getAttribute("data-list-kind")).toBe("ordered");
    expect(lines[6]?.getAttribute("data-list-marker")).toBe("1.");
    expect(lines[6]?.textContent).toBe("Ordered item");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("reveals list source markers only on the active list line", () => {
    const doc = "- First item\n- Second item\n\nEdit";
    const view = createView({ doc, anchor: doc.indexOf("First") });
    const lines = [...view.dom.querySelectorAll<HTMLElement>(".cm-line")];

    expect(lines[0]?.getAttribute("data-markra-list-source")).toBe("visible");
    expect(lines[0]?.textContent).toBe("- First item");
    expect(lines[1]?.getAttribute("data-markra-list-source")).toBe("hidden");
    expect(lines[1]?.textContent).toBe("Second item");
  });

  it("preserves visual list depth without leaving source indentation in preview text", () => {
    const doc = [
      "- First bullet",
      "- Second bullet",
      "  - Nested bullet",
      "1. Ordered item",
      "- [ ] First task",
      "- [x] Second task",
      "",
      "Edit",
    ].join("\n");
    const view = createView({ doc, anchor: doc.length });
    const lines = [
      ...view.dom.querySelectorAll<HTMLElement>(".cm-markra-list-item"),
    ];

    expect(lines.map((line) => line.getAttribute("data-list-depth"))).toEqual([
      "0",
      "0",
      "1",
      "0",
      "0",
      "0",
    ]);
    expect(lines.map((line) => line.textContent)).toEqual([
      "First bullet",
      "Second bullet",
      "Nested bullet",
      "Ordered item",
      "First task",
      "Second task",
    ]);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("keeps blank source lines inside fenced code blocks at full line height", () => {
    const doc = [
      "~~~ts",
      "const first = 1;",
      "",
      "const second = 2;",
      "~~~",
      "",
      "Edit",
    ].join("\n");
    const view = createView({ doc, anchor: doc.length });
    const lines = [...view.dom.querySelectorAll<HTMLElement>(".cm-line")];

    expect(lines[2]?.classList.contains("cm-markra-empty-line")).toBe(false);
    expect(lines[5]?.classList.contains("cm-markra-empty-line")).toBe(true);
  });
});
