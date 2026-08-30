import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "./markraTestDom";
import {
    isAutoUntitledMarkdownName,
    isGeneratedUntitledMarkdownTitle,
    isMarkdownTitleEditing,
    MarkdownTitleComposition,
    syncMarkdownTitleEditable,
    syncMarkdownTitleElement,
    syncMarkdownTitlePresentation,
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

test("recognizes only generated untitled Markdown file names", () => {
    assert.equal(isAutoUntitledMarkdownName("未命名", "未命名"), true);
    assert.equal(isAutoUntitledMarkdownName("未命名 12", "未命名"), true);
    assert.equal(isAutoUntitledMarkdownName("未命名文档", "未命名"), false);
    assert.equal(isAutoUntitledMarkdownName("项目说明", "未命名"), false);
});

test("treats matching generated Front Matter titles as placeholders", () => {
    assert.equal(isGeneratedUntitledMarkdownTitle("未命名 12", "未命名 12", "未命名"), true);
    assert.equal(isGeneratedUntitledMarkdownTitle("未命名 12", undefined, "未命名"), true);
    assert.equal(isGeneratedUntitledMarkdownTitle("未命名 12", "真实标题", "未命名"), false);
    assert.equal(isGeneratedUntitledMarkdownTitle("真实标题", "真实标题", "未命名"), false);
});

test("renders generated untitled names as a placeholder until a real title exists", () => {
    const title = createTitle("未命名 12");
    assert.equal(syncMarkdownTitlePresentation(title, "未命名 12", "未命名文档", true), "updated");
    assert.equal(title.textContent, "");
    assert.equal(title.getAttribute("placeholder"), "未命名文档");
    assert.equal(syncMarkdownTitlePresentation(title, "项目标题", "未命名文档", false), "updated");
    assert.equal(title.textContent, "项目标题");
    assert.equal(title.hasAttribute("placeholder"), false);
});
