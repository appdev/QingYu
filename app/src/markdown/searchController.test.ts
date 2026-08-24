import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorView} from "@codemirror/view";
import {EditorState} from "@codemirror/state";
import {codeMirrorSearchPlugin, getCodeMirrorSearchState} from "./markra-core/codemirror";
import {installMarkdownTestDom} from "./markraTestDom";
import {MarkdownSearchController} from "./searchController";

let cleanup: () => void;
let view: EditorView;
let controller: MarkdownSearchController;
let viewUpdateCount: number;
beforeEach(() => {
    cleanup = installMarkdownTestDom();
    Object.assign(window, {siyuan: {languages: {
        close: "Close", next: "Next", previous: "Previous", replace: "Replace",
        replaceAll: "Replace all", search: "Search",
    }}});
});
afterEach(() => { controller?.destroy(); view?.destroy(); cleanup(); });

const setup = (readOnly = false) => {
    const container = document.body.appendChild(document.createElement("div"));
    viewUpdateCount = 0;
    view = new EditorView({doc: "one TWO one", parent: container, extensions: [
        codeMirrorSearchPlugin(), EditorState.readOnly.of(readOnly), EditorView.updateListener.of((update) => {
            viewUpdateCount++;
            if (viewUpdateCount > 20) throw new Error("Search decorations recursively refreshed");
            controller?.refreshAfterViewUpdate(update);
        }),
    ]});
    controller = new MarkdownSearchController(view, container);
    controller.open(true);
    const query = container.querySelector<HTMLInputElement>('[data-type="query"]');
    query.value = "one";
    query.dispatchEvent(new Event("input"));
    return container;
};

test("finds, wraps, and replaces current-document matches", () => {
    const container = setup();
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 2);
    controller.next(1);
    container.querySelector<HTMLInputElement>('[data-type="replacement"]').value = "x";
    assert.equal(controller.replaceCurrent(), true);
    assert.equal(view.state.doc.toString(), "one TWO x");
    assert.equal(controller.replaceAll(), true);
    assert.equal(view.state.doc.toString(), "x TWO x");
});

test("does not replace a read-only document", () => {
    setup(true);
    controller.next(1);
    assert.equal(controller.replaceCurrent(), false);
    assert.equal(controller.replaceAll(), false);
});

test("preserves the document selection, updates case-sensitive matches, and restores editor focus on close", () => {
    const container = setup();
    view.dispatch({selection: {anchor: 4, head: 7}});
    controller.open(false);
    assert.deepEqual(view.state.selection.main.toJSON(), {anchor: 4, head: 7});
    const query = container.querySelector<HTMLInputElement>('[data-type="query"]');
    query.value = "two";
    const caseSensitive = container.querySelector<HTMLInputElement>('[data-type="case"]');
    caseSensitive.checked = true;
    caseSensitive.dispatchEvent(new Event("change"));
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 0);
    caseSensitive.checked = false;
    caseSensitive.dispatchEvent(new Event("change"));
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 1);
    controller.close();
    assert.equal(document.activeElement, view.contentDOM);
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 0);
});

test("refreshes after document changes, contains Escape, and removes its DOM on destroy", () => {
    const container = setup();
    view.dispatch({changes: {from: view.state.doc.length, insert: " one"}});
    controller.refresh();
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 3);
    const query = container.querySelector<HTMLInputElement>('[data-type="query"]');
    let bubbled = false;
    container.addEventListener("keydown", () => bubbled = true);
    const escape = new KeyboardEvent("keydown", {bubbles: true, cancelable: true, key: "Escape"});
    query.dispatchEvent(escape);
    assert.equal(escape.defaultPrevented, true);
    assert.equal(bubbled, false);
    controller.destroy();
    assert.equal(container.querySelector(".markdown-editor__search"), null);
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 0);
});

test("does not recursively refresh search decoration transactions", () => {
    const container = setup();
    const initialUpdateCount = viewUpdateCount;
    const query = container.querySelector<HTMLInputElement>('[data-type="query"]');
    query.value = "TWO";
    query.dispatchEvent(new Event("input"));
    assert.equal(viewUpdateCount, initialUpdateCount + 1);
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 1);
    view.dispatch({changes: {from: view.state.doc.length, insert: " TWO"}});
    assert.equal(viewUpdateCount, initialUpdateCount + 3);
    assert.equal(getCodeMirrorSearchState(view.state).matches.length, 2);
});
