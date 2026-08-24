import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "./markraTestDom";
import {
    isMarkdownTitleEditing,
    MarkdownTitleComposition,
    syncMarkdownTitleEditable,
    syncMarkdownTitleElement,
} from "./titleEditing";

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    cleanup();
});

const createTitle = (text = "Original") => {
    const title = document.createElement("div");
    title.setAttribute("contenteditable", "true");
    title.tabIndex = 0;
    title.textContent = text;
    document.body.append(title);
    return title;
};

test("does not rewrite an actively edited title after an asynchronous rename", () => {
    const title = createTitle();
    title.focus();
    const selection = document.getSelection();
    const range = document.createRange();
    range.setStart(title.firstChild as Text, 4);
    range.collapse(true);
    selection?.removeAllRanges();
    selection?.addRange(range);
    const originalTextNode = title.firstChild;

    assert.equal(isMarkdownTitleEditing(title), true);
    assert.equal(syncMarkdownTitleElement(title, "Server title"), "deferred");
    assert.equal(title.textContent, "Original");
    assert.equal(title.firstChild, originalTextNode);
    assert.equal(selection?.anchorOffset, 4);
});

test("updates an inactive title and avoids rewriting equal text", () => {
    const title = createTitle();
    assert.equal(syncMarkdownTitleElement(title, "Renamed"), "updated");
    const renamedTextNode = title.firstChild;
    assert.equal(syncMarkdownTitleElement(title, "Renamed"), "unchanged");
    assert.equal(title.firstChild, renamedTextNode);
});

test("commits only outside an active IME composition", () => {
    const composition = new MarkdownTitleComposition();
    composition.start();
    assert.equal(composition.acceptsInput({isComposing: true}), false);
    assert.equal(composition.acceptsInput({isComposing: false}), false);
    assert.equal(composition.acceptsKeydown({isComposing: true}), false);
    composition.end();
    assert.equal(composition.acceptsInput({isComposing: false}), true);
    assert.equal(composition.acceptsKeydown({isComposing: false}), true);
});

test("does not rewrite an unchanged contenteditable state", () => {
    const title = createTitle();
    assert.equal(syncMarkdownTitleEditable(title, true), false);
    assert.equal(syncMarkdownTitleEditable(title, false), true);
    assert.equal(title.getAttribute("contenteditable"), "false");
});
