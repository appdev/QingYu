import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {Compartment, EditorSelection, EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {runScopeHandlers} from "@codemirror/view";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {installMarkdownTestDom} from "./markraTestDom";

const adapter: MarkdownHostAdapter = {
    createIcon: () => document.createElementNS("http://www.w3.org/2000/svg", "svg"),
    notifyError: () => undefined,
    openLink: () => undefined,
    positionPopover: () => undefined,
    renderMath: () => document.createElement("span"),
    renderMermaid: async () => document.createElement("div"),
    resolveImageSource: (source) => source,
    saveClipboardAssets: async () => [],
};

let cleanup: () => void;
let view: EditorView | undefined;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    view?.destroy();
    view = undefined;
    cleanup();
});

const createView = (doc: string, anchor: number, mode: "source" | "visual" = "visual") => {
    view = new EditorView({
        doc,
        extensions: [
            minimalSetup,
            EditorState.allowMultipleSelections.of(true),
            createSiyuanMarkraExtension({
                adapter,
                documentPath: () => "/test.md",
                mode,
            }),
        ],
        parent: document.body,
        selection: {anchor},
    });
    return view;
};

const pressSelectAll = (editor: EditorView) => runScopeHandlers(editor, new KeyboardEvent("keydown", {
    bubbles: true,
    key: "a",
    metaKey: true,
}), "editor");

test("selects the current logical line before the whole Markdown document", () => {
    const editor = createView("first line\nsecond line\nthird line", 15);

    assert.equal(pressSelectAll(editor), true);
    assert.deepEqual({from: editor.state.selection.main.from, to: editor.state.selection.main.to}, {from: 11, to: 22});

    assert.equal(pressSelectAll(editor), true);
    assert.deepEqual({from: editor.state.selection.main.from, to: editor.state.selection.main.to}, {
        from: 0,
        to: editor.state.doc.length,
    });
});

test("uses logical lines for lists, blank lines, trailing lines, and fenced code", () => {
    const cases = [
        {anchor: 4, doc: "  - item\nnext", expected: "  - item"},
        {anchor: 6, doc: "alpha\n\nomega", expected: ""},
        {anchor: 6, doc: "alpha\n", expected: ""},
        {anchor: 9, doc: "```ts\nconst value = 1;\n```", expected: "const value = 1;"},
    ];

    for (const item of cases) {
        view?.destroy();
        const editor = createView(item.doc, item.anchor);
        assert.equal(pressSelectAll(editor), true);
        assert.equal(editor.state.sliceDoc(editor.state.selection.main.from, editor.state.selection.main.to), item.expected);
    }
});

test("resets the select-all escalation after selection changes, edits, and blur", () => {
    const editor = createView("alpha\nbeta", 1);
    pressSelectAll(editor);
    editor.dispatch({selection: EditorSelection.cursor(7)});
    pressSelectAll(editor);
    assert.equal(editor.state.sliceDoc(editor.state.selection.main.from, editor.state.selection.main.to), "beta");

    editor.dispatch({changes: {from: editor.state.doc.length, insert: "!"}});
    pressSelectAll(editor);
    assert.equal(editor.state.sliceDoc(editor.state.selection.main.from, editor.state.selection.main.to), "beta!");

    editor.contentDOM.dispatchEvent(new FocusEvent("blur"));
    editor.dispatch({selection: EditorSelection.cursor(1)});
    pressSelectAll(editor);
    assert.equal(editor.state.sliceDoc(editor.state.selection.main.from, editor.state.selection.main.to), "alpha");
});

test("falls back to the whole document for multiple selections", () => {
    const editor = createView("alpha\nbeta", 0);
    editor.dispatch({selection: EditorSelection.create([EditorSelection.cursor(0), EditorSelection.cursor(6)])});

    pressSelectAll(editor);

    assert.deepEqual([editor.state.selection.main.from, editor.state.selection.main.to], [0, editor.state.doc.length]);
});

test("uses the same select-all behavior in source mode", () => {
    const editor = createView("alpha\nbeta", 7, "source");

    pressSelectAll(editor);

    assert.equal(editor.state.sliceDoc(editor.state.selection.main.from, editor.state.selection.main.to), "beta");
});

test("resets select-all escalation when the Markdown mode changes", () => {
    const mode = new Compartment();
    view = new EditorView({
        doc: "alpha\nbeta",
        extensions: [minimalSetup, mode.of(createSiyuanMarkraExtension({
            adapter,
            documentPath: () => "/test.md",
            mode: "visual",
        }))],
        parent: document.body,
        selection: {anchor: 1},
    });
    pressSelectAll(view);

    view.dispatch({effects: mode.reconfigure(createSiyuanMarkraExtension({
        adapter,
        documentPath: () => "/test.md",
        mode: "source",
    }))});
    pressSelectAll(view);

    assert.equal(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to), "alpha");
});
