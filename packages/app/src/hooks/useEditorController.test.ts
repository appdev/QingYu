import {
  markdownImageInsertionForSelection,
  markdownLinkInsertionForSelection,
  scrollElementToContainerTop,
  selectionAnchorFromEditorView,
  useEditorController
} from "./useEditorController";
import { act, renderHook } from "@testing-library/react";
import {
  defaultValueCtx,
  Editor,
  editorViewCtx,
  remarkStringifyOptionsCtx,
  rootCtx,
  serializerCtx
} from "@milkdown/kit/core";
import { commonmark } from "@milkdown/kit/preset/commonmark";
import { gfm } from "@milkdown/kit/preset/gfm";
import { TextSelection } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";
import { markraTaskListSchema } from "@markra/editor";

function rect(overrides: Partial<DOMRect> = {}): DOMRect {
  return {
    bottom: 0,
    height: 0,
    left: 0,
    right: 0,
    top: 0,
    width: 0,
    x: 0,
    y: 0,
    toJSON: () => ({}),
    ...overrides
  };
}

type MockOutlineNode = {
  attrs: { level: number };
  position: number;
  textContent: string;
  type: { name: string };
};

function mockOutlineHeading(position: number, level: number, textContent: string): MockOutlineNode {
  return {
    attrs: { level },
    position,
    textContent,
    type: { name: "heading" }
  };
}

function mockOutlineEditor(view: EditorView): Editor {
  return {
    action: (runner: (ctx: { get: () => EditorView }) => unknown) => runner({
      get: () => view
    })
  } as unknown as Editor;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("editor controller scrolling", () => {
  it("keeps outline jumps below the fixed titlebar", () => {
    const container = document.createElement("div");
    const target = document.createElement("h2");
    const scrollTo = vi.fn();

    Object.defineProperty(container, "scrollTop", {
      configurable: true,
      value: 100
    });
    Object.defineProperty(container, "scrollTo", {
      configurable: true,
      value: scrollTo
    });
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue(rect({ height: 700, top: 0 }));
    vi.spyOn(target, "getBoundingClientRect").mockReturnValue(rect({ height: 48, top: 240 }));

    scrollElementToContainerTop(target, container);

    expect(scrollTo).toHaveBeenCalledWith({
      behavior: "auto",
      top: 276
    });
  });

});

describe("editor controller outline navigation", () => {
  it("selects outline headings by their non-empty outline index", () => {
    const paperScroll = document.createElement("div");
    const editorDom = document.createElement("div");
    const targetHeading = document.createElement("h4");
    const scrollTo = vi.fn();
    const selection = { synthetic: "selection" };
    const transaction = { synthetic: "transaction" };
    const setSelection = vi.fn(() => transaction);
    const dispatch = vi.fn();
    const focus = vi.fn();
    const nodeDOM = vi.fn(() => targetHeading);
    const resolve = vi.fn(() => ({ synthetic: "resolved-position" }));
    const nodes = [
      mockOutlineHeading(0, 4, "One"),
      mockOutlineHeading(20, 4, "Two"),
      mockOutlineHeading(40, 4, ""),
      mockOutlineHeading(60, 4, "Three")
    ];

    paperScroll.className = "paper-scroll";
    paperScroll.append(editorDom);
    Object.defineProperty(paperScroll, "scrollTop", {
      configurable: true,
      value: 0
    });
    Object.defineProperty(paperScroll, "scrollTo", {
      configurable: true,
      value: scrollTo
    });
    vi.spyOn(paperScroll, "getBoundingClientRect").mockReturnValue(rect({ height: 700, top: 0 }));
    vi.spyOn(targetHeading, "getBoundingClientRect").mockReturnValue(rect({ height: 32, top: 120 }));
    vi.spyOn(TextSelection, "near").mockReturnValue(selection as never);

    const view = {
      dispatch,
      dom: editorDom,
      focus,
      nodeDOM,
      state: {
        doc: {
          descendants(callback: (node: MockOutlineNode, position: number) => boolean | undefined) {
            for (const node of nodes) {
              callback(node, node.position);
            }
          },
          resolve
        },
        tr: {
          setSelection
        }
      }
    } as unknown as EditorView;
    const { result } = renderHook(() => useEditorController());

    act(() => result.current.handleEditorReady(mockOutlineEditor(view)));
    act(() => result.current.selectOutlineItem({ level: 4, title: "Three" }, 2));

    expect(resolve).toHaveBeenCalledWith(61);
    expect(TextSelection.near).toHaveBeenCalledWith({ synthetic: "resolved-position" });
    expect(setSelection).toHaveBeenCalledWith(selection);
    expect(dispatch).toHaveBeenCalledWith(transaction);
    expect(nodeDOM).toHaveBeenCalledWith(60);
    expect(scrollTo).toHaveBeenCalledWith({
      behavior: "auto",
      top: 56
    });
    expect(focus).toHaveBeenCalled();
  });
});

describe("editor controller link insertion", () => {
  it("uses a selected URL as both the link label and href", () => {
    expect(markdownLinkInsertionForSelection("https://example.test/articles/about")).toEqual({
      href: "https://example.test/articles/about",
      kind: "link",
      label: "https://example.test/articles/about",
      selectionFromOffset: 0,
      selectionToOffset: "https://example.test/articles/about".length
    });
  });

  it("keeps non-URL selections as an editable markdown link snippet", () => {
    expect(markdownLinkInsertionForSelection("Synthetic label")).toEqual({
      insertedText: "[Synthetic label](https://)",
      kind: "snippet",
      selectionFromOffset: 1,
      selectionToOffset: "[Synthetic label".length
    });
  });

  it("places the cursor after the placeholder label for empty link snippets", () => {
    expect(markdownLinkInsertionForSelection("")).toEqual({
      cursorOffset: "[text".length,
      insertedText: "[text](https://)",
      kind: "snippet"
    });
  });
});

describe("editor controller image insertion", () => {
  it("uses a local asset path placeholder and selects the image source", () => {
    expect(markdownImageInsertionForSelection("Synthetic alt")).toEqual({
      alt: "Synthetic alt",
      insertedText: "![Synthetic alt](assets/image.png)",
      selectionFromOffset: "![Synthetic alt](".length,
      selectionToOffset: "![Synthetic alt](assets/image.png".length,
      src: "assets/image.png"
    });
  });

  it("escapes selected text before using it as image alt markdown", () => {
    const insertion = markdownImageInsertionForSelection(String.raw`A ] bracket \ slash`);

    expect(insertion).toEqual({
      alt: String.raw`A ] bracket \ slash`,
      insertedText: String.raw`![A \] bracket \\ slash](assets/image.png)`,
      selectionFromOffset: String.raw`![A \] bracket \\ slash](`.length,
      selectionToOffset: String.raw`![A \] bracket \\ slash](assets/image.png`.length,
      src: "assets/image.png"
    });
  });
});

describe("editor controller task-list conversion", () => {
  it("delegates to the editor task-list transaction command", async () => {
    const root = document.createElement("div");
    document.body.append(root);
    const editor = await Editor.make()
      .config((ctx) => {
        ctx.set(rootCtx, root);
        ctx.set(defaultValueCtx, "Task item");
        ctx.update(remarkStringifyOptionsCtx, (options) => ({
          ...options,
          bullet: "-" as const
        }));
      })
      .use(commonmark)
      .use(gfm)
      .use(markraTaskListSchema)
      .create();
    const view = editor.action((ctx) => ctx.get(editorViewCtx));
    const { result, unmount } = renderHook(() => useEditorController());

    try {
      act(() => result.current.handleEditorReady(editor));
      act(() => {
        view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 1)));
      });

      let handled = false;
      act(() => {
        handled = result.current.toggleTaskList();
      });

      expect(handled).toBe(true);
      expect(view.state.doc.firstChild?.firstChild?.attrs.checked).toBe(false);
      expect(editor.action((ctx) => ctx.get(serializerCtx)(view.state.doc))).toBe("- [ ] Task item\n");
      expect(result.current.getSelectionFormattingState().actions).toContain("taskList");
      expect(result.current.getSelectionFormattingState().actions).not.toContain("bulletList");
    } finally {
      unmount();
      await editor.destroy();
      root.remove();
    }
  });
});

describe("editor controller selection anchor", () => {
  it("reads the toolbar anchor from the editor selection when DOM focus moves elsewhere", () => {
    const host = document.createElement("p");
    const text = document.createTextNode("Selected text");
    const range = document.createRange();

    host.append(text);
    vi.spyOn(document, "createRange").mockReturnValue(range);
    vi.spyOn(range, "getClientRects").mockReturnValue([
      rect({ bottom: 80, height: 20, left: 40, right: 120, top: 60, width: 80 }),
      rect({ bottom: 104, height: 20, left: 40, right: 180, top: 84, width: 140 })
    ] as unknown as DOMRectList);

    const view = {
      dom: host,
      domAtPos: (position: number) => ({
        node: text,
        offset: position
      }),
      state: {
        selection: {
          empty: false,
          from: 0,
          to: 13
        }
      }
    } as unknown as EditorView;

    expect(selectionAnchorFromEditorView(view)).toEqual({
      bottom: 104,
      left: 40,
      right: 180,
      top: 60
    });
  });
});
