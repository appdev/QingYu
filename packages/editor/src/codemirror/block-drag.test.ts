import { history, undo } from "@codemirror/commands";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import {
  codeMirrorBlockDragPlugin,
  moveCodeMirrorBlock,
  readCodeMirrorBlockRanges,
} from "./block-drag.ts";
import { horizontalRulePlugin } from "./horizontal-rule.ts";
import { getMarkraSlashMenuState, liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string, readOnly = false) {
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

  it("does not render mutation controls in a read-only editor", () => {
    const view = createView("First\n\nSecond", true);

    expect(view.dom.querySelector(".markra-block-drag-handle")).toBeNull();
    const [first, second] = readCodeMirrorBlockRanges(view.state);
    expect(first && second && moveCodeMirrorBlock(view, first.from, second.from, "after")).toBe(false);
    expect(view.state.doc.toString()).toBe("First\n\nSecond");
  });
});
