import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorState, StateEffect, Transaction} from "@codemirror/state";
import {history, undo, redo} from "@codemirror/commands";
import {EditorView} from "@codemirror/view";
import {installMarkdownTestDom} from "../markraTestDom";
import {markraLanguage} from "../markra-core/codemirror";
import {imageAtomicEditingPlugin, selectImageAtomicRange} from "../markra-core/codemirror/image-atomic";
import {liveMarkdown} from "../markra-core/codemirror";
import {annotationExtension, annotationField, changeAnnotation} from "../annotations/extension";
import {anchorAt} from "../annotations/anchors";
import {annotationQuoteText} from "../annotations/sidebar";
import {captureContext, isContextCurrent, type ActionHost} from "./context";
import {executeAction, readActionState} from "./commands";
import {createSelectionMenu} from "./menu";
import {mountSelectionActions} from "./index";
import {createSiyuanMarkraExtension} from "../markraExtension";
import {createTestHostAdapter} from "../appearance/testSupport";

let cleanup: () => void;
const extraGlobals = ["InputEvent", "HTMLTableCellElement", "DataTransfer", "ClipboardEvent"] as const;
const previousGlobals = new Map<string, PropertyDescriptor | undefined>();
const views: EditorView[] = [];
const host: ActionHost = {mode: () => "visual", isAlive: () => true, addAnnotation: () => true,
    report: () => {}, requestLink: async () => "https://example.com"};
beforeEach(() => {
    cleanup = installMarkdownTestDom();
    extraGlobals.forEach((name) => {
        previousGlobals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
        Object.defineProperty(globalThis, name, {configurable: true, value: window[name]});
    });
    Object.assign(window, {siyuan: {languages: require("../../../appearance/langs/en.json")}});
});
afterEach(() => {
    views.splice(0).forEach((view) => view.destroy());
    extraGlobals.forEach((name) => {
        if (previousGlobals.get(name)) Object.defineProperty(globalThis, name, previousGlobals.get(name));
        else Reflect.deleteProperty(globalThis, name);
    });
    cleanup();
});
const editor = (doc: string, from = 0, to = doc.length) => {
    const view = new EditorView({parent: document.body, state: EditorState.create({doc,
        selection: {anchor: from, head: to}, extensions: [markraLanguage, history(), annotationExtension()]})});
    views.push(view);
    return view;
};
const context = (view: EditorView) => captureContext(view, view.contentDOM, "visual");

test("rejects outside controls and stale editor selections", () => {
    const view = editor("甲乙");
    assert.equal(captureContext(view, document.body, "visual"), null);
    const captured = context(view);
    view.dispatch({selection: {anchor: 1}});
    assert.equal(isContextCurrent(captured), false);
});

test("formats around a smaller annotation and preserves note edits across history", async () => {
    const view = editor("甲乙丙丁戊", 1, 4);
    view.dispatch({effects: changeAnnotation.of({...anchorAt(view.state.doc.toString(), 2, 3), id: "a", note: "原备注", createdAt: 1, updatedAt: 1}),
        annotations: Transaction.addToHistory.of(false)});
    assert.equal(await executeAction("bold", context(view), host), "done");
    assert.equal(view.state.doc.toString(), "甲**乙丙丁**戊");
    assert.equal(view.state.field(annotationField)[0].status, "attached");
    view.dispatch({effects: changeAnnotation.of({...view.state.field(annotationField)[0], note: "新备注"}), annotations: Transaction.addToHistory.of(false)});
    assert.ok(undo(view));
    assert.equal(view.state.field(annotationField)[0].from, 2);
    assert.equal(view.state.field(annotationField)[0].note, "新备注");
    assert.ok(redo(view));
    assert.equal(annotationQuoteText(view.state, view.state.field(annotationField)[0]), "丙");
});

test("keeps cross-paragraph annotations available while disabling formatting", async () => {
    const view = editor("甲\n\n乙");
    assert.equal(readActionState("annotation", context(view)).enabled, true);
    assert.equal(await executeAction("bold", context(view), host), "disabled");
});

test("formatting a captured selection preserves subsequent annotation edits", async () => {
    const view = editor("甲乙");
    const captured = context(view);
    const record = {...anchorAt("甲乙", 0, 2), id: "latest", note: "最新备注", createdAt: 1, updatedAt: 1};
    view.dispatch({effects: changeAnnotation.of(record), annotations: Transaction.addToHistory.of(false)});
    assert.equal(await executeAction("bold", captured, host), "done");
    assert.equal(view.state.field(annotationField)[0].note, "最新备注");
    assert.equal(view.state.field(annotationField)[0].status, "attached");
});

test("link dialog preserves annotation edits and respects a new readonly state", async () => {
    const view = editor("甲乙");
    assert.equal(await executeAction("link", context(view), {...host, requestLink: async () => {
        view.dispatch({effects: changeAnnotation.of({...anchorAt("甲乙", 0, 2), id: "latest", note: "弹窗期间备注", createdAt: 1, updatedAt: 1}),
            annotations: Transaction.addToHistory.of(false)});
        return "https://example.com";
    }}), "done");
    assert.equal(view.state.field(annotationField)[0].note, "弹窗期间备注");
    const locked = editor("甲乙");
    assert.equal(await executeAction("link", context(locked), {...host, requestLink: async () => {
        locked.dispatch({effects: StateEffect.appendConfig.of(EditorState.readOnly.of(true))});
        return "https://example.com";
    }}), "disabled");
    assert.equal(locked.state.doc.toString(), "甲乙");
});

test("clears only the selected part of a bold span", async () => {
    const view = editor("**甲乙丙**", 3, 4);
    assert.equal(await executeAction("clear", context(view), host), "done");
    assert.equal(view.state.doc.toString(), "**甲**乙**丙**");
    assert.ok(undo(view));
    assert.equal(view.state.doc.toString(), "**甲乙丙**");
});

test("toggles a partially selected bold span and clears nested markers in order", async () => {
    const view = editor("**甲乙丙**", 3, 4);
    assert.equal(await executeAction("bold", context(view), host), "done");
    assert.equal(view.state.doc.toString(), "**甲**乙**丙**");
    const nested = editor("**==甲乙丙==**", 5, 6);
    assert.equal(await executeAction("clear", context(nested), host), "done");
    assert.equal(nested.state.doc.toString(), "**==甲==**乙**==丙==**");
});

test("edits an existing link destination without replacing the annotated label", async () => {
    const view = editor("[甲乙](https://old.example)", 1, 3);
    let address = "";
    assert.equal(await executeAction("link", context(view), {...host, requestLink: async (initial) => {
        address = initial; return "https://new.example";
    }}), "done");
    assert.equal(address, "https://old.example");
    assert.equal(view.state.doc.toString(), "[甲乙](https://new.example)");
});

test("table text paste commits through the cell without touching the outer selection", async () => {
    const view = new EditorView({parent: document.body, state: EditorState.create({doc: "| A | B |\n| --- | --- |\n| one | two |",
        extensions: [history(), annotationExtension(), createSiyuanMarkraExtension({mode: "visual", adapter: createTestHostAdapter(), documentPath: () => "/test.md"})]})});
    views.push(view);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const cell = view.dom.querySelector<HTMLElement>("tbody td");
    const range = document.createRange();
    range.selectNodeContents(cell);
    document.getSelection().removeAllRanges();
    document.getSelection().addRange(range);
    const captured = captureContext(view, cell, "visual");
    assert.equal(captured.kind, "table-cell");
    assert.equal(readActionState("bold", captured).enabled, false);
    Object.defineProperty(navigator, "clipboard", {configurable: true, value: {readText: async () => "**new**"}});
    assert.equal(await executeAction("paste-plain", captured, host), "done");
    assert.match(view.state.doc.toString(), /\| \*\*new\*\* \| two \|/u);
    assert.match(view.state.doc.toString(), /^\| A \| B \|/u);
    class TestTransfer {
        private data = new Map<string, string>();
        files: File[] = [];
        items = {add: (file: File) => this.files.push(file)};
        get types() { return Array.from(this.data.keys()); }
        setData(type: string, value: string) { this.data.set(type, value); }
        getData(type: string) { return this.data.get(type) || ""; }
    }
    class TestClipboardEvent extends Event {
        readonly clipboardData: TestTransfer;
        constructor(type: string, options: EventInit & {clipboardData: TestTransfer}) {
            super(type, options);
            this.clipboardData = options.clipboardData;
        }
    }
    Object.defineProperty(globalThis, "DataTransfer", {configurable: true, value: TestTransfer});
    Object.defineProperty(globalThis, "ClipboardEvent", {configurable: true, value: TestClipboardEvent});
    Object.defineProperty(navigator, "clipboard", {value: {read: async () => [{types: ["text/html", "text/plain"],
        getType: async (type: string) => new Blob([type === "text/html" ? "<strong>rich</strong>" : "rich"], {type})}]}});
    const updatedCell = view.dom.querySelector<HTMLElement>("tbody td");
    const nextRange = document.createRange();
    nextRange.selectNodeContents(updatedCell);
    document.getSelection().removeAllRanges();
    document.getSelection().addRange(nextRange);
    assert.equal(await executeAction("paste", captureContext(view, updatedCell, "visual"), host), "done");
    assert.match(view.state.doc.toString(), /\| \*\*rich\*\* \| two \|/u);
});

test("adds code with safe delimiters and rejects unsafe math", async () => {
    const view = editor("a`b");
    assert.equal(await executeAction("code", context(view), host), "done");
    assert.equal(view.state.doc.toString(), "``a`b``");
    const math = editor("a$b");
    assert.equal(await executeAction("math", context(math), host), "disabled");
});

test("clearing formula delimiters preserves literal TeX punctuation", async () => {
    const view = editor("$a*b*$", 1, 5);
    assert.equal(await executeAction("clear", context(view), host), "done");
    assert.equal(view.state.doc.toString(), "a*b*");
});

test("link cancellation and a changing selection never modify the document", async () => {
    const view = editor("甲乙");
    assert.equal(await executeAction("link", context(view), {...host, requestLink: async () => null}), "cancelled");
    assert.equal(await executeAction("link", context(view), {...host, requestLink: async () => {
        view.dispatch({selection: {anchor: 1}}); return "https://example.com";
    }}), "stale");
    assert.equal(view.state.doc.toString(), "甲乙");
});

test("copy rejection never deletes text; successful cut is undoable", async () => {
    const view = editor("甲乙");
    Object.defineProperty(navigator, "clipboard", {configurable: true, value: {writeText: async () => { throw new Error("denied"); }}});
    assert.equal(await executeAction("cut", context(view), host), "failed");
    assert.equal(view.state.doc.toString(), "甲乙");
    Object.defineProperty(navigator, "clipboard", {value: {writeText: async () => {}}});
    assert.equal(await executeAction("cut", context(view), host), "done");
    assert.equal(view.state.doc.toString(), "");
    assert.ok(undo(view));
    assert.equal(view.state.doc.toString(), "甲乙");
});

test("copying a selected image preserves its Markdown and plain alt text", async () => {
    const doc = "![示例](image.png)";
    const view = new EditorView({parent: document.body, state: EditorState.create({doc,
        extensions: liveMarkdown({plugins: [imageAtomicEditingPlugin()]})})});
    views.push(view);
    selectImageAtomicRange(view, {from: 0, to: doc.length});
    const image = document.createElement("img");
    view.contentDOM.append(image);
    const captured = captureContext(view, image, "visual");
    let copied = "";
    Object.defineProperty(navigator, "clipboard", {configurable: true, value: {writeText: async (text: string) => { copied = text; }}});
    assert.equal(await executeAction("copy", captured, host), "done");
    assert.equal(copied, doc);
    assert.equal(await executeAction("copy-plain", captured, host), "done");
    assert.equal(copied, "示例");
    assert.equal(readActionState("annotation", captured).enabled, false);
});

test("a selected text formatting shortcut uses the guarded command", async () => {
    const view = editor("**甲乙丙**", 3, 4);
    const actions = mountSelectionActions(view, host);
    view.focus();
    view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", {key: "b", code: "KeyB", ctrlKey: true, bubbles: true, cancelable: true}));
    assert.equal(view.state.doc.toString(), "**甲**乙**丙**");
    actions.destroy();
});

test("plain and escaped paste have different source semantics", async () => {
    Object.defineProperty(navigator, "clipboard", {configurable: true, value: {readText: async () => "**文字**"}});
    const plain = editor("");
    assert.equal(await executeAction("paste-plain", context(plain), host), "done");
    assert.equal(plain.state.doc.toString(), "**文字**");
    const escaped = editor("");
    assert.equal(await executeAction("paste-escaped", context(escaped), host), "done");
    assert.equal(escaped.state.doc.toString(), "\\*\\*文字\\*\\*");
});

test("menu and toolbar share command availability and destroy their DOM", () => {
    const view = editor("甲乙");
    const items = createSelectionMenu(context(view), host);
    assert.deepEqual(items.filter((item) => item.id).map((item) => item.id), ["markdown-copy", "markdown-copy-plain", "markdown-cut", "markdown-delete",
        "markdown-paste", "markdown-paste-plain", "markdown-paste-escaped", "markdown-select-all"]);
    view.focus();
    const actions = mountSelectionActions(view, host);
    actions.refresh();
    assert.equal(document.querySelectorAll(".markdown-selection-toolbar").length, 1);
    assert.equal(document.querySelectorAll(".markdown-annotation-toolbar").length, 0);
    actions.destroy();
    assert.equal(document.querySelectorAll(".markdown-selection-toolbar").length, 0);
});

test("keyboard focus can enter the toolbar and Escape returns to the editor", async () => {
    const view = editor("甲乙");
    const actions = mountSelectionActions(view, host);
    view.focus();
    view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", {key: "F10", code: "F10", altKey: true, bubbles: true, cancelable: true}));
    await Promise.resolve();
    assert.ok(document.activeElement.closest(".markdown-selection-toolbar"));
    document.activeElement.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true, cancelable: true}));
    assert.equal(view.hasFocus, true);
    assert.equal(document.querySelector<HTMLElement>(".markdown-selection-toolbar").hidden, true);
    actions.destroy();
});
