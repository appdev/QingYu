import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {indentUnit} from "@codemirror/language";
import {EditorSelection, EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {installMarkdownTestDom} from "../../markraTestDom";
import {liveMarkdown, markdownEditingPlugin} from "./index";

let cleanup: () => void;
let view: EditorView | undefined;
beforeEach(() => cleanup = installMarkdownTestDom());
afterEach(() => {
    view?.destroy();
    view = undefined;
    cleanup();
});

const pressTab = (shiftKey = false) => view?.contentDOM.dispatchEvent(new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    key: "Tab",
    shiftKey,
}));

test("uses the configured indentation at every plain-text cursor", () => {
    view = new EditorView({
        doc: "plain\nother",
        extensions: [minimalSetup, EditorState.allowMultipleSelections.of(true), indentUnit.of("    "),
            liveMarkdown({plugins: [markdownEditingPlugin()]})],
        parent: document.body,
        selection: EditorSelection.create([EditorSelection.cursor(0), EditorSelection.cursor(6)]),
    });
    pressTab();
    assert.equal(view.state.doc.toString(), "    plain\n    other");
    assert.deepEqual(view.state.selection.ranges.map((range) => range.head), [4, 14]);
});

test("uses the configured indentation before a list marker", () => {
    view = new EditorView({
        doc: "- item",
        extensions: [minimalSetup, indentUnit.of("    "), liveMarkdown({plugins: [markdownEditingPlugin()]})],
        parent: document.body,
        selection: {anchor: 3},
    });
    pressTab();
    assert.equal(view.state.doc.toString(), "    - item");
});

test("Shift-Tab removes at most one configured list indentation unit", () => {
    view = new EditorView({
        doc: "    - item",
        extensions: [minimalSetup, indentUnit.of("    "), liveMarkdown({plugins: [markdownEditingPlugin()]})],
        parent: document.body,
        selection: {anchor: 6},
    });
    pressTab(true);
    assert.equal(view.state.doc.toString(), "- item");
});
