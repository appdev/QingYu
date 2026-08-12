import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {undo} from "@codemirror/commands";
import {markdown} from "@codemirror/lang-markdown";
import {EditorSelection, EditorState, type Extension} from "@codemirror/state";
import {GFM} from "@lezer/markdown";
import {EditorView, minimalSetup} from "codemirror";
import {runScopeHandlers} from "@codemirror/view";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {getSelectedImageAtomicRange, imageAtomicEditingPlugin} from "./markra-core/codemirror";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {installMarkdownTestDom} from "./markraTestDom";

const adapter: MarkdownHostAdapter = {
    createIcon(_name, className, ownerDocument) {
        const icon = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
        icon.classList.add(className);
        return icon;
    },
    notifyError() {},
    openLink() {},
    positionPopover() {},
    renderMath(_source, _displayMode, context) {
        return context.ownerDocument.createElement("span");
    },
    async renderMermaid(_source, context) {
        return context.ownerDocument.createElement("div");
    },
    resolveImageSource(source) {
        return source;
    },
    async saveClipboardAssets() {
        return [];
    },
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

const createView = (doc: string, options: {extensions?: Extension[], mode?: "source" | "visual"} = {}) => {
    view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc,
            extensions: [
                minimalSetup,
                ...(options.extensions ?? []),
                createSiyuanMarkraExtension({
                    adapter,
                    documentPath: () => "/test.md",
                    mode: options.mode ?? "visual",
                }),
            ],
        }),
    });
    return view;
};

const runDeleteKey = (editor: EditorView, key: "Backspace" | "Delete") => runScopeHandlers(editor, new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    key,
}), "editor");

test("selects an image at the backward boundary before deleting its complete Markdown", () => {
    const source = "前文\n![图](assets/a.png){width=430px}\n后文";
    const imageTo = source.indexOf("\n后文");
    const editor = createView(source);
    editor.dispatch({selection: {anchor: imageTo}});

    assert.equal(runDeleteKey(editor, "Backspace"), true);
    assert.equal(editor.state.doc.toString(), source);
    assert.equal(editor.dom.querySelectorAll(".markra-image-node-selected").length, 1);

    assert.equal(runDeleteKey(editor, "Backspace"), true);
    assert.equal(editor.state.doc.toString(), "前文\n\n后文");
    assert.equal(undo(editor), true);
    assert.equal(editor.state.doc.toString(), source);
});

test("treats a cursor mapped inside a visual image replacement as the image boundary", () => {
    const source = "![图](assets/a.png){width=430px}";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: source.indexOf("{")}});

    assert.equal(runDeleteKey(editor, "Backspace"), true);
    assert.equal(editor.state.doc.toString(), source);
    assert.deepEqual(getSelectedImageAtomicRange(editor.state), {from: 0, to: source.length});

    assert.equal(runDeleteKey(editor, "Backspace"), true);
    assert.equal(editor.state.doc.toString(), "");
});

test("protects a valid width attribute while the syntax tree is missing its custom attribute node", () => {
    const source = "![图](assets/a.png){width=430px}";
    const plugin = imageAtomicEditingPlugin();
    view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc: source,
            extensions: [minimalSetup, markdown({extensions: [GFM]}), plugin.extension ?? []],
            selection: {anchor: source.length},
        }),
    });

    assert.equal(runDeleteKey(view, "Backspace"), true);
    assert.equal(view.state.doc.toString(), source);
    assert.deepEqual(getSelectedImageAtomicRange(view.state), {from: 0, to: source.length});
});

test("selects an image at the forward boundary before deleting it", () => {
    const source = "![图](assets/a.png){width=430px}\n后文";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: 0}});

    assert.equal(runDeleteKey(editor, "Delete"), true);
    assert.equal(editor.state.doc.toString(), source);
    assert.equal(runDeleteKey(editor, "Delete"), true);
    assert.equal(editor.state.doc.toString(), "\n后文");
});

test("deletes multiple adjacent images in one undoable transaction", () => {
    const first = "![图一](assets/a.png){width=430px}";
    const second = "![图二](assets/b.png){width=320px}";
    const source = `${first}\n${second}`;
    const editor = createView(source, {extensions: [EditorState.allowMultipleSelections.of(true)]});
    editor.dispatch({
        selection: EditorSelection.create([
            EditorSelection.cursor(first.length),
            EditorSelection.cursor(source.length),
        ]),
    });

    assert.equal(runDeleteKey(editor, "Backspace"), true);
    assert.equal(editor.state.doc.toString(), "\n");
    assert.equal(undo(editor), true);
    assert.equal(editor.state.doc.toString(), source);
});

test("does not protect source mode or modify read-only visual documents", () => {
    const source = "![图](assets/a.png){width=430px}";
    const sourceEditor = createView(source, {mode: "source"});
    sourceEditor.dispatch({selection: {anchor: source.length}});
    assert.equal(runDeleteKey(sourceEditor, "Backspace"), true);
    assert.equal(sourceEditor.state.doc.toString(), source.slice(0, -1));
    sourceEditor.destroy();

    const readOnlyEditor = createView(source, {extensions: [EditorState.readOnly.of(true)]});
    readOnlyEditor.dispatch({selection: {anchor: source.length}});
    runDeleteKey(readOnlyEditor, "Backspace");
    assert.equal(readOnlyEditor.state.doc.toString(), source);
    assert.equal(getSelectedImageAtomicRange(readOnlyEditor.state), null);
});

test("expands a partial attribute deletion to the complete image", () => {
    const source = "![图](assets/a.png){width=430px}";
    const editor = createView(source);

    editor.dispatch({
        changes: {from: source.length - 1, to: source.length},
        userEvent: "delete.backward",
    });

    assert.equal(editor.state.doc.toString(), "");
    assert.equal(undo(editor), true);
    assert.equal(editor.state.doc.toString(), source);
});

test("copies and cuts the complete Markdown for a clicked image", async () => {
    const source = "![图](assets/a.png){width=430px}";
    const editor = createView(source);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    const image = editor.dom.querySelector(".markra-image-node") as HTMLElement;
    image.dispatchEvent(new MouseEvent("click", {bubbles: true, cancelable: true}));
    assert.deepEqual(getSelectedImageAtomicRange(editor.state), {from: 0, to: source.length});
    assert.equal(runDeleteKey(editor, "Backspace"), true);
    assert.equal(editor.state.doc.toString(), "");
    assert.equal(undo(editor), true);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    (editor.dom.querySelector(".markra-image-node") as HTMLElement).dispatchEvent(
        new MouseEvent("click", {bubbles: true, cancelable: true}),
    );

    let copied = "";
    const clipboardData = {
        clearData() {
            copied = "";
        },
        getData(type: string) {
            return type === "text/plain" ? copied : "";
        },
        setData(type: string, value: string) {
            if (type === "text/plain") copied = value;
        },
    };
    const copyEvent = new Event("copy", {bubbles: true, cancelable: true});
    Object.defineProperty(copyEvent, "clipboardData", {value: clipboardData});
    editor.contentDOM.dispatchEvent(copyEvent);
    assert.equal(copied, source);
    assert.equal(editor.state.doc.toString(), source);

    const cutEvent = new Event("cut", {bubbles: true, cancelable: true});
    Object.defineProperty(cutEvent, "clipboardData", {value: clipboardData});
    editor.contentDOM.dispatchEvent(cutEvent);
    assert.equal(copied, source);
    assert.equal(editor.state.doc.toString(), "");
});

test("uses the same two-step deletion for mobile beforeinput", () => {
    const source = "![图](assets/a.png){width=430px}";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: source.length}});

    const dispatchDelete = () => {
        const event = new Event("beforeinput", {bubbles: true, cancelable: true});
        Object.defineProperties(event, {
            inputType: {value: "deleteContentBackward"},
            isComposing: {value: false},
        });
        editor.contentDOM.dispatchEvent(event);
    };

    dispatchDelete();
    assert.equal(editor.state.doc.toString(), source);
    dispatchDelete();
    assert.equal(editor.state.doc.toString(), "");
});
