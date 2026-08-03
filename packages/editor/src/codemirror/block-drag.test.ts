// @vitest-environment jsdom
import { history, undo } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import {
  ensureSyntaxTree,
  forceParsing,
  syntaxTreeAvailable,
} from "@codemirror/language";
import { EditorSelection, EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  codeMirrorBlockDragPlugin,
  moveCodeMirrorBlock,
  readCodeMirrorBlockRanges,
} from "./block-drag.ts";
import { horizontalRulePlugin } from "./horizontal-rule.ts";
import { getMarkraSlashMenuState, liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const synchronousParseRequests = vi.hoisted(
  (): Array<{
    kind: "ensure" | "force";
    timeout: number | undefined;
    upto: number;
  }> => [],
);
const syntaxTreeIterations = vi.hoisted(
  (): Array<{ from: number | undefined; to: number | undefined }> => [],
);
const suppressedSyntaxTreeStates = vi.hoisted(() => new WeakSet<object>());
const syntaxTreeProxyCaches = vi.hoisted(() => ({
  normal: new WeakMap<object, object>(),
  suppressed: new WeakMap<object, object>(),
}));

vi.mock("@codemirror/language", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@codemirror/language")>();

  return {
    ...actual,
    ensureSyntaxTree(
      ...args: Parameters<typeof actual.ensureSyntaxTree>
    ) {
      synchronousParseRequests.push({
        kind: "ensure",
        timeout: args[2],
        upto: args[1],
      });
      return actual.ensureSyntaxTree(...args);
    },
    forceParsing(...args: Parameters<typeof actual.forceParsing>) {
      synchronousParseRequests.push({
        kind: "force",
        timeout: args[2],
        upto: args[1] ?? args[0].viewport.to,
      });
      return actual.forceParsing(...args);
    },
    syntaxTree(state: Parameters<typeof actual.syntaxTree>[0]) {
      const tree = actual.syntaxTree(state);
      const suppressed = suppressedSyntaxTreeStates.has(state);
      const cache = suppressed
        ? syntaxTreeProxyCaches.suppressed
        : syntaxTreeProxyCaches.normal;
      const cached = cache.get(tree);
      if (cached) return cached as typeof tree;
      const proxy = new Proxy(tree, {
        get(target, property, receiver) {
          if (property !== "iterate") {
            return Reflect.get(target, property, receiver);
          }

          return (spec: Parameters<typeof tree.iterate>[0]) => {
            if (suppressed) return;
            syntaxTreeIterations.push({ from: spec.from, to: spec.to });
            return target.iterate(spec);
          };
        },
      });
      cache.set(tree, proxy);
      return proxy;
    },
  };
});

const views: EditorView[] = [];

function createView(
  doc: string,
  readOnly = false,
  extensions: readonly Extension[] = [],
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        history(),
        liveMarkdown({
          plugins: [codeMirrorBlockDragPlugin(), horizontalRulePlugin()],
          slashMenu: true,
        }),
        EditorState.readOnly.of(readOnly),
        ...extensions,
      ],
      selection: EditorSelection.cursor(doc.length),
    }),
  });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
  synchronousParseRequests.splice(0);
  syntaxTreeIterations.splice(0);
});

describe("codeMirrorBlockDragPlugin", () => {
  it("discovers list items as independently draggable blocks without rewriting source", () => {
    const doc = "# Title\n\nParagraph\n\n- One\n- Two\n\n> Quote";
    const view = createView(doc);

    expect(readCodeMirrorBlockRanges(view.state).map((range) =>
      view.state.sliceDoc(range.from, range.to))).toEqual([
      "# Title",
      "Paragraph",
      "- One",
      "- Two",
      "> Quote",
    ]);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("discovers and moves blocks after parsing advances beyond the initial viewport", () => {
    const doc = [
      ...Array.from({ length: 400 }, (_, index) => `Paragraph ${index}`),
      "- Final list item",
    ].join("\n\n");
    const view = createView(doc);

    expect(forceParsing(view, doc.length, 1_000)).toBe(true);
    const finalBlock = readCodeMirrorBlockRanges(view.state).at(-1);
    expect(finalBlock).toMatchObject({
      from: doc.lastIndexOf("- Final list item"),
      name: "ListItem",
    });
    expect(
      finalBlock && moveCodeMirrorBlock(view, finalBlock.from, 0, "before"),
    ).toBe(true);
    expect(view.state.doc.toString().startsWith("- Final list item")).toBe(true);
  });

  it("returns complete exported ranges before the background parser finishes", () => {
    const doc = [
      ...Array.from({ length: 400 }, (_, index) => `Paragraph ${index}`),
      "- Final list item",
    ].join("\n\n");
    const state = EditorState.create({
      doc,
      extensions: [markdown()],
    });

    expect(readCodeMirrorBlockRanges(state).at(-1)).toMatchObject({
      from: doc.lastIndexOf("- Final list item"),
      name: "ListItem",
    });
  });

  it("keeps quoted lists inside the top-level blockquote block", () => {
    const doc = [
      "> - Quoted one",
      "> - Quoted two",
      "",
      "- Top one",
      "- Top two",
    ].join("\n");
    const state = EditorState.create({
      doc,
      extensions: [markdown()],
    });

    expect(readCodeMirrorBlockRanges(state).map((block) => ({
      name: block.name,
      source: state.sliceDoc(block.from, block.to),
    }))).toEqual([
      {
        name: "Blockquote",
        source: "> - Quoted one\n> - Quoted two",
      },
      { name: "ListItem", source: "- Top one" },
      { name: "ListItem", source: "- Top two" },
    ]);
  });

  it("keeps quoted nested lists inside their top-level list item", () => {
    const doc = [
      "- Top",
      "  > - Quoted nested one",
      "  > - Quoted nested two",
      "- Next",
    ].join("\n");
    const state = EditorState.create({
      doc,
      extensions: [markdown()],
    });

    expect(readCodeMirrorBlockRanges(state).map((block) => ({
      name: block.name,
      source: state.sliceDoc(block.from, block.to),
    }))).toEqual([
      {
        name: "ListItem",
        source: "- Top\n  > - Quoted nested one\n  > - Quoted nested two",
      },
      { name: "ListItem", source: "- Next" },
    ]);
  });

  it("ignores a completed parser context until its tree is published", () => {
    const doc = [
      ...Array.from({ length: 400 }, (_, index) => `Paragraph ${index}`),
      "- Final list item",
    ].join("\n\n");
    const state = EditorState.create({
      doc,
      extensions: [markdown()],
    });

    expect(ensureSyntaxTree(state, doc.length, 1_000)).not.toBeNull();
    expect(syntaxTreeAvailable(state)).toBe(true);
    expect(readCodeMirrorBlockRanges(state).at(-1)).toMatchObject({
      from: doc.lastIndexOf("- Final list item"),
      name: "ListItem",
    });
  });

  it("does not synchronously request full parsing from the doc-change decoration path", () => {
    const doc = Array.from(
      { length: 10_000 },
      (_, index) => `Paragraph ${index}`,
    ).join("\n\n");
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc,
        extensions: [
          markdown(),
          codeMirrorBlockDragPlugin().extension ?? [],
        ],
      }),
    });
    views.push(view);

    synchronousParseRequests.splice(0);
    syntaxTreeIterations.splice(0);
    view.dispatch({ changes: { from: 0, insert: "Edited " } });
    const visibleRanges = view.visibleRanges.map(({ from, to }) => ({
      from,
      to,
    }));

    expect(synchronousParseRequests).toEqual([]);
    expect(syntaxTreeIterations.length).toBeGreaterThan(0);
    expect(syntaxTreeIterations.every((iteration) =>
      visibleRanges.some((visibleRange) =>
        iteration.from === visibleRange.from &&
        iteration.to === visibleRange.to
      ))).toBe(true);
  });

  it("rebuilds block decorations only after the parser publishes a new tree", () => {
    const doc = [
      "- Visible item",
      ...Array.from({ length: 400 }, (_, index) => `Paragraph ${index}`),
    ].join("\n\n");
    const plugin = codeMirrorBlockDragPlugin();
    const stableState = EditorState.create({
      doc,
      extensions: [markdown(), plugin.extension ?? []],
    });
    const stableParent = document.createElement("div");
    document.body.append(stableParent);
    const stableView = new EditorView({
      parent: stableParent,
      state: stableState,
    });
    views.push(stableView);
    syntaxTreeIterations.splice(0);
    stableView.dispatch({});
    expect(syntaxTreeIterations).toHaveLength(0);

    const deferredState = EditorState.create({
      doc,
      extensions: [markdown(), plugin.extension ?? []],
    });
    suppressedSyntaxTreeStates.add(deferredState);
    const deferredParent = document.createElement("div");
    document.body.append(deferredParent);
    const deferredView = new EditorView({
      parent: deferredParent,
      state: deferredState,
    });
    views.push(deferredView);
    expect(
      deferredView.dom.querySelector('[data-markra-block-from="0"]'),
    ).toBeNull();

    syntaxTreeIterations.splice(0);
    expect(forceParsing(deferredView, doc.length, 1_000)).toBe(true);
    expect(syntaxTreeIterations).toEqual(
      deferredView.visibleRanges.map(({ from, to }) => ({ from, to })),
    );
    expect(
      deferredView.dom.querySelector('[data-markra-block-from="0"]'),
    ).not.toBeNull();

    syntaxTreeIterations.splice(0);
    deferredView.dispatch({});
    expect(syntaxTreeIterations).toHaveLength(0);
  });

  it("reorders one list item without moving the entire list", () => {
    const doc = "- First\n- Second\n- Third\n\nAfter";
    const view = createView(doc);
    const [first, second] = readCodeMirrorBlockRanges(view.state);

    expect(first?.name).toBe("ListItem");
    expect(second?.name).toBe("ListItem");
    expect(second && first && moveCodeMirrorBlock(view, second.from, first.from, "before")).toBe(true);
    expect(view.state.doc.toString()).toBe("- Second\n- First\n- Third\n\nAfter");
  });

  it("moves a paragraph into a list as a sibling item", () => {
    const doc = "Paragraph\n\n- First\n- Second";
    const view = createView(doc);
    const [paragraph, first] = readCodeMirrorBlockRanges(view.state);

    expect(paragraph && first && moveCodeMirrorBlock(view, paragraph.from, first.from, "after")).toBe(true);
    expect(view.state.doc.toString()).toBe("- First\n- Paragraph\n- Second");
  });

  it("outdents a nested list item when a shallower drop depth is requested", () => {
    const doc = "- First\n  - Nested\n- Last";
    const view = createView(doc);
    const nested = readCodeMirrorBlockRanges(view.state).find(
      (block) => block.depth === 1,
    );
    const last = readCodeMirrorBlockRanges(view.state).at(-1);

    expect(nested && last && moveCodeMirrorBlock(view, nested.from, last.from, "before", 0)).toBe(true);
    expect(view.state.doc.toString()).toBe("- First\n- Nested\n- Last");
  });

  it("preserves parsed child-item styling by changing only the moved region", () => {
    const doc = [
      "- **First title**: Body",
      "- Middle item",
      "- **Second title**: Body",
      "- **Third title**: Body",
      "",
      "After",
    ].join("\n");
    const changedRanges: Array<{ from: number; to: number }> = [];
    const view = createView(doc, false, [
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        update.changes.iterChangedRanges((from, to) => {
          changedRanges.push({ from, to });
        });
      }),
    ]);
    const [first, , second] = readCodeMirrorBlockRanges(view.state);

    expect(
      first && second &&
        moveCodeMirrorBlock(view, second.from, first.from, "after", 1),
    ).toBe(true);
    expect(view.state.doc.toString()).toBe([
      "- **First title**: Body",
      "  - **Second title**: Body",
      "- Middle item",
      "- **Third title**: Body",
      "",
      "After",
    ].join("\n"));
    expect(changedRanges).toHaveLength(1);
    expect(changedRanges[0]?.from).toBeGreaterThan(0);
    expect(changedRanges[0]?.to).toBeLessThan(doc.length);
    expect(forceParsing(view, view.state.doc.length, 1_000)).toBe(true);

    const childLine = Array.from(
      view.dom.querySelectorAll<HTMLElement>(".cm-markra-list-item"),
    ).find((line) => line.textContent === "Second title: Body");
    expect(childLine?.getAttribute("data-list-depth")).toBe("1");
    expect(childLine?.getAttribute("data-markra-list-source")).toBe("hidden");
  });

  it("clamps a child drop to the deepest available parent level", () => {
    const view = createView([
      "- **First title**: Body",
      "- Middle item",
      "- **Moved title**: Body",
      "- Last item",
    ].join("\n"));
    const [first, , source] = readCodeMirrorBlockRanges(view.state);

    expect(
      source && first && moveCodeMirrorBlock(
        view,
        source.from,
        first.from,
        "after",
        3,
      ),
    ).toBe(true);
    expect(view.state.doc.toString()).toBe([
      "- **First title**: Body",
      "  - **Moved title**: Body",
      "- Middle item",
      "- Last item",
    ].join("\n"));
    expect(forceParsing(view, view.state.doc.length, 1_000)).toBe(true);
    const movedLine = Array.from(
      view.dom.querySelectorAll<HTMLElement>(".cm-line"),
    ).find((line) => line.textContent?.includes("Moved title"));
    expect(movedLine?.getAttribute("data-list-depth")).toBe("1");
    expect(movedLine?.getAttribute("data-markra-list-source")).toBe("hidden");
  });

  it("uses the parent marker width when nesting below an ordered item", () => {
    const view = createView([
      "10. Ordered parent",
      "- **Moved title**: Body",
      "- Last item",
    ].join("\n"));
    const [parent, source] = readCodeMirrorBlockRanges(view.state);

    expect(
      source && parent && moveCodeMirrorBlock(
        view,
        source.from,
        parent.from,
        "after",
        1,
      ),
    ).toBe(true);
    expect(view.state.doc.toString()).toBe([
      "10. Ordered parent",
      "    - **Moved title**: Body",
      "- Last item",
    ].join("\n"));
    expect(forceParsing(view, view.state.doc.length, 1_000)).toBe(true);
    const movedLine = Array.from(
      view.dom.querySelectorAll<HTMLElement>(".cm-line"),
    ).find((line) => line.textContent?.includes("Moved title"));
    expect(movedLine?.getAttribute("data-list-depth")).toBe("1");
    expect(movedLine?.getAttribute("data-markra-list-source")).toBe("hidden");
  });

  it("preserves tab-expanded columns when nesting a second-level item deeper", () => {
    const view = createView([
      "- Parent",
      "\t- First child",
      "\t- Second child",
      "\t\t- Grandchild",
      "- Tail",
    ].join("\n"));
    const blocks = readCodeMirrorBlockRanges(view.state);
    const firstChild = blocks.find((block) =>
      view.state.sliceDoc(block.from, block.to).startsWith("\t- First child")
    );
    const secondChild = blocks.find((block) =>
      view.state.sliceDoc(block.from, block.to).startsWith("\t- Second child")
    );

    expect(firstChild?.depth).toBe(1);
    expect(secondChild?.depth).toBe(1);
    expect(
      secondChild && firstChild && moveCodeMirrorBlock(
        view,
        secondChild.from,
        firstChild.from,
        "after",
        2,
      ),
    ).toBe(true);
    expect(view.state.doc.toString()).toBe([
      "- Parent",
      "\t- First child",
      "      - Second child",
      "          - Grandchild",
      "- Tail",
    ].join("\n"));
    const movedBlocks = readCodeMirrorBlockRanges(view.state);
    const moved = movedBlocks.find((block) =>
      view.state.doc.lineAt(block.from).text.includes("Second child")
    );
    const grandchild = movedBlocks.find((block) =>
      view.state.doc.lineAt(block.from).text.includes("Grandchild")
    );
    expect(moved?.depth).toBe(2);
    expect(grandchild?.depth).toBe(3);
  });

  it("turns a paragraph dropped as a child into a nested list item", () => {
    const view = createView("- Parent\n\nChild paragraph\n\nAfter");
    const [parent, child] = readCodeMirrorBlockRanges(view.state);

    expect(
      child && parent && moveCodeMirrorBlock(
        view,
        child.from,
        parent.from,
        "after",
        1,
      ),
    ).toBe(true);
    expect(view.state.doc.toString()).toBe(
      "- Parent\n  - Child paragraph\n\nAfter",
    );
    expect(forceParsing(view, view.state.doc.length, 1_000)).toBe(true);
    const childLine = Array.from(
      view.dom.querySelectorAll<HTMLElement>(".cm-line"),
    ).find((line) => line.textContent === "Child paragraph");
    expect(childLine?.getAttribute("data-list-depth")).toBe("1");
    expect(childLine?.getAttribute("data-markra-list-source")).toBe("hidden");
  });

  it("renders block controls with the app-compatible icon structure", () => {
    const view = createView("First\n\nSecond");
    const add = view.dom.querySelector(".markra-block-add-button");
    const drag = view.dom.querySelector(".markra-block-drag-handle");

    expect(add?.classList.contains("markra-block-tool-button")).toBe(true);
    expect(drag?.classList.contains("markra-block-tool-button")).toBe(true);
    expect(drag?.querySelectorAll(".markra-block-drag-dot")).toHaveLength(6);
  });

  it("moves a top-level block as one undoable source edit", () => {
    const doc = "First\n\nSecond\n\nThird";
    const view = createView(doc);
    const [first, second] = readCodeMirrorBlockRanges(view.state);

    expect(first && second && moveCodeMirrorBlock(view, first.from, second.from, "after")).toBe(true);
    expect(view.state.doc.toString()).toBe("Second\n\nFirst\n\nThird");
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("reorders a four-asterisk horizontal rule as one block", () => {
    const doc = "First\n\n****\n\nSecond";
    const view = createView(doc);
    const blocks = readCodeMirrorBlockRanges(view.state);
    const rule = blocks.find((block) => block.name === "HorizontalRule");
    const second = blocks.find((block) => view.state.sliceDoc(block.from, block.to) === "Second");

    expect(view.dom.querySelectorAll("hr.cm-markra-horizontal-rule")).toHaveLength(1);
    expect(rule && second && moveCodeMirrorBlock(view, rule.from, second.from, "after")).toBe(true);
    expect(view.state.doc.toString()).toBe("First\n\nSecond\n\n****");
  });

  it("reorders blocks through the rendered drag handle", () => {
    const view = createView("First\n\nSecond\n\nThird");
    const [first, second] = readCodeMirrorBlockRanges(view.state);
    const handle = view.dom.querySelector<HTMLElement>(
      `[data-block-from="${first?.from}"] .markra-block-drag-handle`,
    );
    const target = view.dom.querySelector<HTMLElement>(
      `.cm-line[data-markra-block-from="${second?.from}"]`,
    );
    const values = new Map<string, string>();
    const dataTransfer = {
      dropEffect: "none",
      effectAllowed: "none",
      getData: (type: string) => values.get(type) ?? "",
      setData: (type: string, value: string) => values.set(type, value),
      types: ["application/x-markra-codemirror-block"],
    };
    const dragStart = new MouseEvent("dragstart", { bubbles: true, cancelable: true });
    Object.defineProperty(dragStart, "dataTransfer", { value: dataTransfer });
    const dragOver = new MouseEvent("dragover", { bubbles: true, cancelable: true });
    Object.defineProperty(dragOver, "dataTransfer", { value: dataTransfer });
    const drop = new MouseEvent("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: dataTransfer });

    handle?.dispatchEvent(dragStart);
    target?.dispatchEvent(dragOver);
    expect(view.dom.querySelector(".markra-block-drag-source")).not.toBeNull();
    expect(view.dom.querySelector(".markra-block-drop-indicator")?.getAttribute("data-show")).toBe("true");
    target?.dispatchEvent(drop);

    expect(drop.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe("Second\n\nFirst\n\nThird");
    expect(view.dom.querySelector(".markra-block-drag-source")).toBeNull();
    expect(view.dom.querySelector(".markra-block-drop-indicator")).toBeNull();
  });

  it("allows a protected internal dragover before dropping the offset-zero block", () => {
    const view = createView("First\n\nSecond\n\nThird");
    const [first, second] = readCodeMirrorBlockRanges(view.state);
    const handle = view.dom.querySelector<HTMLElement>(
      `[data-block-from="${first?.from}"] .markra-block-drag-handle`,
    );
    const target = view.dom.querySelector<HTMLElement>(
      `.cm-line[data-markra-block-from="${second?.from}"]`,
    );
    const dragStart = new MouseEvent("dragstart", { bubbles: true, cancelable: true });
    Object.defineProperty(dragStart, "dataTransfer", {
      value: {
        effectAllowed: "none",
        setData: vi.fn(),
      },
    });
    const dragOver = new MouseEvent("dragover", { bubbles: true, cancelable: true });
    Object.defineProperty(dragOver, "dataTransfer", {
      value: {
        dropEffect: "none",
        getData: () => "",
        types: {
          0: "application/x-markra-codemirror-block",
          contains: (type: string) => type === "application/x-markra-codemirror-block",
          item: (index: number) => index === 0 ? "application/x-markra-codemirror-block" : null,
          length: 1,
        },
      },
    });
    const drop = new MouseEvent("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", {
      value: {
        getData: (type: string) => type === "application/x-markra-codemirror-block" ? "0" : "",
        types: ["application/x-markra-codemirror-block"],
      },
    });

    handle?.dispatchEvent(dragStart);
    target?.dispatchEvent(dragOver);

    expect(dragOver.defaultPrevented).toBe(true);
    expect(view.dom.querySelector(".markra-block-drop-indicator")?.getAttribute("data-show")).toBe("true");

    target?.dispatchEvent(drop);

    expect(drop.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe("Second\n\nFirst\n\nThird");
  });

  it("ignores external drops without block drag data when Front Matter is present", () => {
    const doc = "---\ntitle: Native\n---\n\nFirst\n\nSecond";
    const view = createView(doc);
    const second = readCodeMirrorBlockRanges(view.state).find(
      (block) => view.state.sliceDoc(block.from, block.to) === "Second",
    );
    const target = view.dom.querySelector<HTMLElement>(
      `.cm-line[data-markra-block-from="${second?.from}"]`,
    );
    const dragOver = new MouseEvent("dragover", { bubbles: true, cancelable: true });
    Object.defineProperty(dragOver, "dataTransfer", {
      value: { getData: () => "", types: [] },
    });
    const drop = new MouseEvent("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", {
      value: { getData: () => "", types: [] },
    });

    target?.dispatchEvent(dragOver);
    target?.dispatchEvent(drop);

    expect(dragOver.defaultPrevented).toBe(false);
    expect(drop.defaultPrevented).toBe(false);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("reorders task items through pointer dragging when native drag events are unavailable", () => {
    const view = createView(
      "- [ ] First task\n- [ ] Second task\n- [ ] Third task",
    );
    const [first, second] = readCodeMirrorBlockRanges(view.state);
    const handle = view.dom.querySelector<HTMLElement>(
      `[data-block-from="${first?.from}"] .markra-block-drag-handle`,
    );
    const target = view.dom.querySelector<HTMLElement>(
      `.cm-line[data-markra-block-from="${second?.from}"]`,
    );

    handle?.dispatchEvent(new PointerEvent("pointerdown", {
      bubbles: true,
      button: 0,
      buttons: 1,
      clientX: 10,
      clientY: 10,
      pointerId: 1,
    }));
    target?.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true,
      buttons: 1,
      clientX: 20,
      clientY: 40,
      pointerId: 1,
    }));

    expect(view.dom.querySelector(".markra-block-drag-source")).not.toBeNull();
    expect(
      view.dom.querySelector(".markra-block-drop-indicator")?.getAttribute(
        "data-show",
      ),
    ).toBe("true");

    target?.dispatchEvent(new PointerEvent("pointerup", {
      bubbles: true,
      button: 0,
      clientX: 20,
      clientY: 40,
      pointerId: 1,
    }));

    expect(view.state.doc.toString()).toBe(
      "- [ ] Second task\n- [ ] First task\n- [ ] Third task",
    );
    expect(view.dom.querySelector(".markra-block-drag-source")).toBeNull();
    expect(view.dom.querySelector(".markra-block-drop-indicator")).toBeNull();
  });

  it("nests a second-level item as a third-level item through pointer dragging", () => {
    const view = createView([
      "- Parent",
      "  - First child",
      "  - Second child",
      "- Tail",
    ].join("\n"));
    const blocks = readCodeMirrorBlockRanges(view.state);
    const firstChild = blocks.find((block) =>
      view.state.sliceDoc(block.from, block.to).startsWith("  - First child")
    );
    const secondChild = blocks.find((block) =>
      view.state.sliceDoc(block.from, block.to).startsWith("  - Second child")
    );
    const handle = view.dom.querySelector<HTMLElement>(
      `[data-block-from="${secondChild?.from}"] .markra-block-drag-handle`,
    );
    const target = view.dom.querySelector<HTMLElement>(
      `.cm-line[data-markra-block-from="${firstChild?.from}"]`,
    );

    handle?.dispatchEvent(new PointerEvent("pointerdown", {
      bubbles: true,
      button: 0,
      buttons: 1,
      clientX: 44,
      clientY: 10,
      pointerId: 2,
    }));
    target?.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true,
      buttons: 1,
      clientX: 66,
      clientY: 40,
      pointerId: 2,
    }));
    target?.dispatchEvent(new PointerEvent("pointerup", {
      bubbles: true,
      button: 0,
      clientX: 66,
      clientY: 40,
      pointerId: 2,
    }));

    expect(view.state.doc.toString()).toBe([
      "- Parent",
      "  - First child",
      "    - Second child",
      "- Tail",
    ].join("\n"));
    const moved = readCodeMirrorBlockRanges(view.state).find((block) =>
      view.state.sliceDoc(block.from, block.to).startsWith("    - Second child")
    );
    expect(moved?.depth).toBe(2);
  });

  it("adds an editable blank block below and opens the virtual slash menu", () => {
    const view = createView("First\n\nSecond\n\nThird");
    const second = readCodeMirrorBlockRanges(view.state)[1];
    const button = view.dom.querySelector<HTMLButtonElement>(
      `[data-block-from="${second?.from}"] [aria-label="Add block below"]`,
    );

    button?.click();

    expect(view.state.doc.toString()).toBe("First\n\nSecond\n\n\n\nThird");
    expect(getMarkraSlashMenuState(view)).toMatchObject({
      open: true,
      source: "virtual",
    });
    expect(view.dom.querySelectorAll(".markra-block-add-button")).toHaveLength(4);
  });

  it("keeps later block controls mounted while typing plain text before them", () => {
    const doc = "Edit here\n\n## Later block";
    const view = createView(doc);
    const laterFrom = doc.indexOf("## Later block");
    const laterToolbar = view.dom.querySelector<HTMLElement>(
      `[data-block-from="${laterFrom}"]`,
    );

    view.dispatch({
      changes: { from: "Edit here".length, insert: "字" },
      selection: EditorSelection.cursor("Edit here字".length),
      userEvent: "input.type",
    });

    const nextLaterFrom = laterFrom + 1;
    expect(
      view.dom.querySelector(`[data-block-from="${nextLaterFrom}"]`),
    ).toBe(laterToolbar);
    expect(
      view.dom.querySelector(
        `.cm-line[data-markra-block-from="${nextLaterFrom}"]`,
      ),
    ).not.toBeNull();
  });

  it("does not render mutation controls in a read-only editor", () => {
    const view = createView("First\n\nSecond", true);

    expect(view.dom.querySelector(".markra-block-drag-handle")).toBeNull();
    const [first, second] = readCodeMirrorBlockRanges(view.state);
    expect(first && second && moveCodeMirrorBlock(view, first.from, second.from, "after")).toBe(false);
    expect(view.state.doc.toString()).toBe("First\n\nSecond");
  });
});
