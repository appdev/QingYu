import { history, undo } from "@codemirror/commands";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import {
  codeMirrorBlockDragPlugin,
  moveCodeMirrorBlock,
  readCodeMirrorBlockRanges,
} from "./block-drag.ts";
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
          plugins: [codeMirrorBlockDragPlugin()],
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
  it("discovers top-level Markdown blocks without rewriting source", () => {
    const doc = "# Title\n\nParagraph\n\n- One\n- Two\n\n> Quote";
    const view = createView(doc);

    expect(readCodeMirrorBlockRanges(view.state).map((range) =>
      view.state.sliceDoc(range.from, range.to))).toEqual([
      "# Title",
      "Paragraph",
      "- One\n- Two",
      "> Quote",
    ]);
    expect(view.state.doc.toString()).toBe(doc);
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
    const drop = new MouseEvent("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: dataTransfer });

    handle?.dispatchEvent(dragStart);
    target?.dispatchEvent(drop);

    expect(drop.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe("Second\n\nFirst\n\nThird");
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
  });

  it("does not render mutation controls in a read-only editor", () => {
    const view = createView("First\n\nSecond", true);

    expect(view.dom.querySelector(".markra-block-drag-handle")).toBeNull();
    const [first, second] = readCodeMirrorBlockRanges(view.state);
    expect(first && second && moveCodeMirrorBlock(view, first.from, second.from, "after")).toBe(false);
    expect(view.state.doc.toString()).toBe("First\n\nSecond");
  });
});
