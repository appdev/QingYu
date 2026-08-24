import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "./markraTestDom";
import {handlePendingPlainTextPasteEvent} from "./markra-core/plain-text-paste";
import {
    executeMarkdownEditorCommand,
    isMarkdownTypewriterShortcut,
    resolveMarkdownShortcut,
    routeMarkdownShortcut,
    requestMarkdownPlainTextPaste,
    type MarkdownCommandTarget,
} from "./editorCommands";

let cleanup: () => void;
beforeEach(() => {
    cleanup = installMarkdownTestDom();
    Object.assign(window, {siyuan: {config: {editor: {rtl: false}}}});
});

test("plain-text paste waits for native fallback on API rejection and ignores non-editor focus", async () => {
    const {editor} = target();
    const content = document.body.appendChild(document.createElement("textarea"));
    editor.view.contentDOM = content;
    const search = document.body.appendChild(document.createElement("input"));
    let reads = 0;
    assert.equal(requestMarkdownPlainTextPaste(editor, search, () => {
        reads += 1;
        return Promise.reject(new Error("denied"));
    }), false);
    assert.equal(reads, 0);

    content.focus();
    assert.equal(requestMarkdownPlainTextPaste(editor, content, () => {
        reads += 1;
        return Promise.reject(new Error("denied"));
    }), false);
    await Promise.resolve();
    assert.equal(reads, 1);
    const fallback = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(fallback, "clipboardData", {value: {
        getData: (type: string) => type === "text/plain" ? "### fallback" : "",
    }});
    assert.equal(handlePendingPlainTextPasteEvent(fallback as ClipboardEvent, content), true);
    assert.equal(content.value, "### fallback");
});

test("successful Clipboard API text clears the pending native fallback", async () => {
    const {editor} = target();
    const content = document.body.appendChild(document.createElement("textarea"));
    editor.view.contentDOM = content;
    content.focus();
    requestMarkdownPlainTextPaste(editor, content, () => Promise.resolve("literal"));
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(content.value, "literal");
    const duplicate = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(duplicate, "clipboardData", {value: {getData: () => "duplicate"}});
    assert.equal(handlePendingPlainTextPasteEvent(duplicate as ClipboardEvent, content), false);
});
afterEach(() => cleanup());

const target = () => {
    const calls: string[] = [];
    const editor: MarkdownCommandTarget = {
        element: document.body,
        view: {contentDOM: document.body},
        isReadOnly: () => false,
        openSearch: (replace) => calls.push(replace ? "replace" : "search"),
        refreshEditorConfig: () => calls.push("refresh"),
        setMode: (mode) => calls.push(mode),
        toggleFullscreen: () => calls.push("fullscreen"),
        toggleTypewriterMode: () => calls.push("typewriter"),
        updateEditorPreference: (key, value) => calls.push(`${key}:${value}`),
    };
    return {calls, editor};
};

test("routes search, replace, and mode commands", () => {
    const {calls, editor} = target();
    assert.equal(executeMarkdownEditorCommand(editor, "search"), true);
    assert.equal(executeMarkdownEditorCommand(editor, "replace"), true);
    assert.equal(executeMarkdownEditorCommand(editor, "source-mode"), true);
    assert.equal(executeMarkdownEditorCommand(editor, "visual-mode"), true);
    assert.deepEqual(calls, ["search", "replace", "source", "visual"]);
});

test("refreshes toggled direction and blocks read-only paste", () => {
    const {calls, editor} = target();
    assert.equal(executeMarkdownEditorCommand(editor, "toggle-rtl"), true);
    assert.deepEqual(calls, ["rtl:true"]);
    editor.isReadOnly = () => true;
    assert.equal(executeMarkdownEditorCommand(editor, "paste-plain-text"), false);
});

test("routes typewriter locally and persists justify through the editor preference API", () => {
    const {calls, editor} = target();
    assert.equal(executeMarkdownEditorCommand(editor, "toggle-typewriter"), true);
    assert.equal(executeMarkdownEditorCommand(editor, "toggle-justify"), true);
    assert.deepEqual(calls, ["typewriter", "justify:true"]);
});

const shortcutKeymap = (active: "replace" | "search" | "fullscreen" | "rtl" | "preview" | "wysiwyg") => ({
    general: {
        replace: {custom: active === "replace" ? "A" : ""},
        search: {custom: active === "search" ? "A" : ""},
    },
    editor: {general: {
        fullscreen: {custom: active === "fullscreen" ? "A" : ""},
        preview: {custom: active === "preview" ? "A" : ""},
        rtl: {custom: active === "rtl" ? "A" : ""},
        wysiwyg: {custom: active === "wysiwyg" ? "A" : ""},
    }},
});

const shortcutEvent = () => {
    const event = new KeyboardEvent("keydown", {bubbles: true, cancelable: true, key: "a"});
    Object.defineProperty(event, "keyCode", {value: 65});
    return event;
};

test("maps every configured Markdown shortcut", () => {
    const expected = new Map([
        ["replace", "replace"],
        ["search", "search"],
        ["fullscreen", "toggle-fullscreen"],
        ["rtl", "toggle-rtl"],
        ["preview", "visual-mode"],
        ["wysiwyg", "visual-mode"],
    ] as const);
    expected.forEach((command, configured) => {
        assert.equal(resolveMarkdownShortcut(shortcutEvent(), shortcutKeymap(configured), (hotkey) => hotkey === "A"), command);
    });
});

test("consumes a routed shortcut before a global handler can repeat it", () => {
    const {calls, editor} = target();
    const event = shortcutEvent();
    let globalCalls = 0;
    document.body.addEventListener("keydown", (nextEvent) => {
        if (!nextEvent.defaultPrevented) globalCalls += 1;
    });
    assert.equal(routeMarkdownShortcut(editor, event, shortcutKeymap("search"), (hotkey) => hotkey === "A"), true);
    document.body.dispatchEvent(event);
    assert.deepEqual(calls, ["search"]);
    assert.equal(event.defaultPrevented, true);
    assert.equal(globalCalls, 0);
});

test("recognizes the Markdown-local typewriter shortcut only for Mod-Shift-Y", () => {
    assert.equal(isMarkdownTypewriterShortcut(new KeyboardEvent("keydown", {ctrlKey: true, shiftKey: true, key: "y"})), true);
    assert.equal(isMarkdownTypewriterShortcut(new KeyboardEvent("keydown", {ctrlKey: true, key: "y"})), false);
    assert.equal(isMarkdownTypewriterShortcut(new KeyboardEvent("keydown", {ctrlKey: true, shiftKey: true, key: "x"})), false);
});
