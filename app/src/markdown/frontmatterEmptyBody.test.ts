import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {runScopeHandlers} from "@codemirror/view";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {installMarkdownTestDom} from "./markraTestDom";
import {createTestHostAdapter} from "./appearance/testSupport";
import {initialVisualMarkdownSelection} from "./markra-core/codemirror/frontmatter-preview";

let cleanup: () => void;
let view: EditorView | undefined;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
    Object.assign(window, {
        siyuan: {
            config: {editor: {}},
            languages: {emptyPlaceholder: "Write something"},
        },
    });
});

afterEach(() => {
    view?.destroy();
    view = undefined;
    cleanup();
});

const mountEditor = (doc: string) => {
    view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc,
            extensions: [minimalSetup, createSiyuanMarkraExtension({
                adapter: createTestHostAdapter(),
                documentPath: () => "/Untitled.md",
                mode: "visual",
            })],
        }),
    });
    return view;
};

test("keeps one normal placeholder line when hidden frontmatter is the only document content", () => {
    const editor = mountEditor("---\ntitle: Untitled\n---\n\n");
    const editableLine = editor.dom.querySelector<HTMLElement>(".cm-markra-empty-body-line");

    assert.ok(editableLine);
    assert.equal(editableLine.matches(".cm-line.cm-markra-empty-line"), true);
    assert.equal(editableLine.querySelector(".cm-placeholder")?.textContent, "Write something");
    assert.equal(editor.dom.querySelectorAll(".cm-markra-empty-body-line").length, 1);
    assert.equal(editor.dom.querySelectorAll(".cm-line").length, 1);
});

test("does not show an empty-body placeholder when visible Markdown content exists", () => {
    const editor = mountEditor("---\ntitle: Untitled\n---\n\nBody");

    assert.equal(editor.dom.querySelector(".cm-markra-empty-body-line"), null);
    assert.equal(editor.dom.querySelector(".cm-placeholder"), null);
});

test("starts the visual cursor after hidden frontmatter and leading body line breaks", () => {
    assert.equal(initialVisualMarkdownSelection("Plain text"), 0);
    assert.equal(initialVisualMarkdownSelection("---\ntitle: Untitled\n---\n\n"), 25);
    assert.equal(initialVisualMarkdownSelection("---\ntitle: Untitled\n---\n\nBody"), 25);
});

test("moves the cursor into the empty body when frontmatter is inserted after editor creation", () => {
    const editor = mountEditor("");
    const source = "---\ntitle: Untitled\n---\n\n";

    editor.dispatch({changes: {from: 0, insert: source}});

    assert.equal(editor.state.selection.main.head, source.length);
    assert.equal(editor.dom.querySelector(".cm-markra-empty-body-line")?.querySelector(".cm-placeholder")?.textContent,
        "Write something");
});

test("backspace in an empty visual body does not delete hidden frontmatter", () => {
    const source = "---\ntitle: Untitled\n---\n\n";
    const editor = mountEditor(source);
    editor.dispatch({selection: {anchor: source.length}});

    const handled = runScopeHandlers(editor, new KeyboardEvent("keydown", {key: "Backspace"}), "editor");

    assert.equal(handled, true);
    assert.equal(editor.state.doc.toString(), source);
    assert.equal(editor.state.selection.main.head, source.length);
});
